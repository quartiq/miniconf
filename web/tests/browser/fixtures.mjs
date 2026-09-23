import { test as base, expect } from "@playwright/test";
import { once } from "node:events";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const requireMqtt = createRequire(import.meta.resolve("mqtt"));
const { WebSocketServer } = requireMqtt("ws");
const packet = requireMqtt("mqtt-packet");
const html = readFileSync("dist/index.html");

export const test = base.extend({
  device: async ({}, use) => {
    const server = createServer((_request, response) => {
      response.setHeader("Content-Type", "text/html");
      response.end(html);
    });
    const broker = new WebSocketServer({ server });
    const prefix = "dt/test/device";
    const schema = `${JSON.stringify({
      s: { ty: "i32", future: ["opaque"] },
      m: {
        note: "first line\nsecond line",
        flag: false,
        empty: null,
        nested: { a: [1, 2] },
        doc: "Signed digital mixer step in 1/16 of the ADC sample rate.\n\nPositive advances the complex oscillator as exp(+j*phase) and shifts the sampled spectrum upward. Reconstructing the analog input therefore subtracts this frequency from the demodulation DDS carrier. The step aliases modulo 16 into the principal interval [-8, 7].",
      },
    })}\n{"i":{"k":"n","c":{"leaf":{"r":0,"m":{"note":"edge note"}},"other":0}}}\n`;
    let revision = 0x811c9dc5;
    for (const byte of new TextEncoder().encode(schema))
      revision = Math.imul(revision ^ byte, 0x01000193) >>> 0;
    const retained = new Map();
    const writes = [];
    const device = { epoch: 1, connections: 0, clearAcks: 0, rejectClearAt: 0 };
    function resetFixture() {
      device.epoch = 1;
      writes.length = 0;
      device.rejectSubscriptions = false;
      device.rejectCleanup = false;
      device.holdResponse = false;
      device.holdSetAck = false;
      device.respond = undefined;
      device.holdClear = false;
      device.acknowledgeClear = undefined;
      device.clearAcks = 0;
      device.rejectClearAt = 0;
      device.holdConnect = false;
      retained.clear();
      retained.set(`${prefix}/alive`, {
        text: JSON.stringify({
          proto: 1,
          epoch: device.epoch,
          schema_rev: revision,
          pages: 1,
        }),
      });
      retained.set(`${prefix}/schema/0`, { text: schema });
      retained.set(`${prefix}/settings/leaf`, {
        text: "9007199254740993",
        properties: { userProperties: { auth: "", rev: "1" } },
      });
      retained.set(`${prefix}/settings/other`, {
        text: "null",
        properties: { userProperties: { auth: "" } },
      });
      for (const ns of ["settings", "set", "response"])
        retained.set(`${prefix}/${ns}/obsolete`, {
          text: "7",
          properties: { userProperties: { auth: "", rev: "999" } },
        });
    }
    function matches(filter, topic) {
      const levels = filter.split("/");
      const parts = topic.split("/");
      return (
        levels.every(
          (level, i) =>
            level === "#" ||
            (parts[i] !== undefined && (level === "+" || level === parts[i])),
        ) &&
        (levels.at(-1) === "#" || levels.length === parts.length)
      );
    }
    function send(socket, topic, entry, retain) {
      if (socket.readyState !== 1) return;
      socket.send(
        packet.generate(
          {
            cmd: "publish",
            qos: 0,
            topic,
            payload: entry.text,
            retain,
            properties: entry.properties,
          },
          { protocolVersion: 5 },
        ),
      );
    }
    function publish(
      topic,
      text,
      properties = { userProperties: { auth: "" } },
    ) {
      const entry = { text, properties };
      if (text) retained.set(topic, entry);
      else retained.delete(topic);
      for (const socket of broker.clients) {
        for (const sub of socket.subscriptions ?? []) {
          if (matches(sub.topic, topic)) {
            send(socket, topic, entry, !!sub.rap);
            break;
          }
        }
      }
    }
    broker.on("connection", (socket) => {
      device.connections++;
      socket.subscriptions = [];
      const parser = packet.parser({ protocolVersion: 5 });
      socket.on("error", () => {});
      socket.on("message", (bytes) => parser.parse(bytes));
      parser.on("packet", (message) => {
        const reply = (value) => {
          if (socket.readyState === 1)
            socket.send(packet.generate(value, { protocolVersion: 5 }));
        };
        if (message.cmd === "connect") {
          device.lastConnect = message;
          const accept = () =>
            reply({
              cmd: "connack",
              reasonCode: message.username === "rejected-user" ? 0x86 : 0,
              sessionPresent: false,
            });
          if (device.holdConnect) broker.emit("held-connect", accept);
          else accept();
        } else if (message.cmd === "subscribe") {
          if (device.rejectSubscriptions) {
            reply({
              cmd: "suback",
              messageId: message.messageId,
              granted: message.subscriptions.map(() => 128),
            });
            return;
          }
          const granted = message.subscriptions.map((sub) =>
            device.rejectCleanup && sub.topic === `${prefix}/set/#` ? 135 : 1,
          );
          socket.subscriptions = message.subscriptions.filter(
            (_sub, index) => granted[index] < 128,
          );
          reply({
            cmd: "suback",
            messageId: message.messageId,
            granted,
          });
          for (const sub of socket.subscriptions)
            if (sub.rh !== 2) {
              for (const [topic, entry] of retained)
                if (matches(sub.topic, topic)) send(socket, topic, entry, true);
            }
        } else if (message.cmd === "publish") {
          writes.push(message);
          const clearing = message.retain && message.payload.length === 0;
          const rejected =
            clearing && ++device.clearAcks === device.rejectClearAt;
          if (clearing && !rejected)
            publish(message.topic, "", message.properties);
          if (message.qos === 1) {
            const acknowledge = () =>
              reply({
                cmd: "puback",
                messageId: message.messageId,
                reasonCode: rejected ? 135 : 0,
              });
            if (clearing && device.holdClear)
              device.acknowledgeClear = acknowledge;
            else if (!device.holdSetAck || clearing) acknowledge();
          }
          if (
            message.topic === `${prefix}/set/leaf` &&
            message.payload.length
          ) {
            device.respond = (
              text = message.payload.toString(),
              echoFirst = true,
              code = "Ok",
            ) => {
              if (code === "Ok" && echoFirst)
                publish(`${prefix}/settings/leaf`, text);
              send(
                socket,
                message.properties.responseTopic,
                {
                  text: code === "Ok" ? "" : text,
                  properties: {
                    correlationData: message.properties.correlationData,
                    userProperties: { code },
                  },
                },
                false,
              );
              if (code === "Ok" && !echoFirst)
                publish(`${prefix}/settings/leaf`, text);
            };
            if (!device.holdResponse) device.respond();
          }
          broker.emit("publication", message);
        } else if (message.cmd === "pingreq") reply({ cmd: "pingresp" });
      });
    });

    resetFixture();
    server.listen(0, "127.0.0.1");
    await once(server, "listening");
    Object.assign(device, {
      broker,
      prefix,
      schema,
      revision,
      retained,
      writes,
      publish,
      resetFixture,
      port: server.address().port,
    });
    try {
      await use(device);
    } finally {
      for (const socket of broker.clients) socket.terminate();
      await new Promise((resolve) => broker.close(resolve));
      await new Promise((resolve) => server.close(resolve));
    }
  },
  baseURL: async ({ device }, use, testInfo) => {
    await use(
      testInfo.project.name === "file"
        ? pathToFileURL(resolve("dist/index.html")).href
        : testInfo.project.name === "dev"
          ? process.env.MINICONF_WEB_DEV_URL
          : `http://127.0.0.1:${device.port}/`,
    );
  },
  browseURL: async ({ baseURL, device }, use) => {
    await use(
      `${baseURL}#/browse/127.0.0.1:${device.port}/${device.prefix}?discover=dt%2Ftest%2F%2B`,
    );
  },
  browsePage: async ({ page, browseURL }, use) => {
    await page.goto(browseURL);
    await page.locator('[data-tree-path="/leaf"]').click();
    await expect(
      page.getByRole("button", { name: "Set", exact: true }),
    ).toBeEnabled();
    await use(page);
  },
  browserErrors: [
    async ({ page }, use) => {
      const errors = [];
      page.on("pageerror", (error) => errors.push(error.message));
      page.on("console", (message) => {
        if (message.type() === "error") errors.push(message.text());
      });
      await use();
      expect(errors).toEqual([]);
    },
    { auto: true },
  ],
});
