import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const requireMqtt = createRequire(import.meta.resolve("mqtt"));
const { WebSocketServer } = requireMqtt("ws");
const packet = requireMqtt("mqtt-packet");
const html = readFileSync("dist/index.html");
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
let epoch = 1;
const retained = new Map();
const writes = [];
let rejectSubscriptions = false;
let rejectCleanup = false;
let connections = 0;
let holdResponse = false;
let holdSetAck = false;
let respond;
let holdClear = false;
let acknowledgeClear;
let clearAcks = 0;
let rejectClearAt = 0;
function seed() {
  retained.clear();
  retained.set(`${prefix}/alive`, {
    text: JSON.stringify({ proto: 1, epoch, schema_rev: revision, pages: 1 }),
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
function publish(topic, text, properties = { userProperties: { auth: "" } }) {
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
  connections++;
  socket.subscriptions = [];
  const parser = packet.parser({ protocolVersion: 5 });
  socket.on("error", () => {});
  socket.on("message", (bytes) => parser.parse(bytes));
  parser.on("packet", (message) => {
    const reply = (value) =>
      socket.send(packet.generate(value, { protocolVersion: 5 }));
    if (message.cmd === "connect")
      reply({ cmd: "connack", reasonCode: 0, sessionPresent: false });
    else if (message.cmd === "subscribe") {
      if (rejectSubscriptions) {
        reply({
          cmd: "suback",
          messageId: message.messageId,
          granted: message.subscriptions.map(() => 128),
        });
        return;
      }
      const granted = message.subscriptions.map((sub) =>
        rejectCleanup && sub.topic === `${prefix}/set/#` ? 135 : 1,
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
      const rejected = clearing && ++clearAcks === rejectClearAt;
      if (clearing && !rejected) publish(message.topic, "", message.properties);
      if (message.qos === 1) {
        const acknowledge = () =>
          reply({
            cmd: "puback",
            messageId: message.messageId,
            reasonCode: rejected ? 135 : 0,
          });
        if (clearing && holdClear) acknowledgeClear = acknowledge;
        else if (!holdSetAck || clearing) acknowledge();
      }
      if (message.topic === `${prefix}/set/leaf` && message.payload.length) {
        respond = () => {
          publish(`${prefix}/settings/leaf`, message.payload.toString());
          send(
            socket,
            message.properties.responseTopic,
            {
              text: "",
              properties: {
                correlationData: message.properties.correlationData,
                userProperties: { code: "Ok" },
              },
            },
            false,
          );
        };
        if (!holdResponse) respond();
      }
    } else if (message.cmd === "pingreq") reply({ cmd: "pingresp" });
  });
});
mkdirSync(".codex", { recursive: true });
const profile = mkdtempSync(resolve(".codex/browser-check-"));
let chrome;
const pending = new Map();
const errors = [];
let sequence = 0;
let sessionId;
function command(method, params = {}, session = sessionId) {
  const id = ++sequence;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`Browser command timed out: ${method}`));
    }, 10_000);
    pending.set(id, { resolve, reject, timer });
    chrome.stdio[3].write(
      JSON.stringify({
        id,
        method,
        params,
        ...(session ? { sessionId: session } : {}),
      }) + "\0",
    );
  });
}
async function evaluate(expression) {
  const response = await command("Runtime.evaluate", {
    expression,
    returnByValue: true,
    awaitPromise: true,
  });
  assert(!response.exceptionDetails, JSON.stringify(response.exceptionDetails));
  return response.result.value;
}
async function until(expression) {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (await evaluate(expression)) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(
    `Browser condition timed out: ${expression}\n${await evaluate("document.body?.innerText")}`,
  );
}
let touch = false;
async function click(selector) {
  const { x, y } = await evaluate(`(() => {
    const element = document.querySelector(${JSON.stringify(selector)});
    element.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    const rect = element.getBoundingClientRect();
    return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 };
  })()`);
  if (touch) {
    await command("Input.dispatchTouchEvent", {
      type: "touchStart",
      touchPoints: [{ x, y }],
    });
    await command("Input.dispatchTouchEvent", {
      type: "touchEnd",
      touchPoints: [],
    });
  } else {
    await command("Input.dispatchMouseEvent", {
      type: "mousePressed",
      x,
      y,
      button: "left",
      clickCount: 1,
    });
    await command("Input.dispatchMouseEvent", {
      type: "mouseReleased",
      x,
      y,
      button: "left",
      clickCount: 1,
    });
  }
  await evaluate(
    "new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))",
  );
}
async function press(key) {
  await command("Input.dispatchKeyEvent", { type: "keyDown", key });
  await command("Input.dispatchKeyEvent", { type: "keyUp", key });
}
async function clickButton(label, scope = "body") {
  await evaluate(`(() => {
    const button = [...document.querySelector(${JSON.stringify(scope)}).querySelectorAll('button')]
      .find(button => button.textContent.trim() === ${JSON.stringify(label)});
    if (!button || button.disabled) throw new Error('Button unavailable: ' + ${JSON.stringify(label)});
    button.click();
  })()`);
  await evaluate(
    "new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))",
  );
}
async function viewport(width, height) {
  touch = width <= 760;
  await command("Emulation.setDeviceMetricsOverride", {
    width,
    height,
    deviceScaleFactor: 1,
    mobile: touch,
  });
  await command("Emulation.setTouchEmulationEnabled", { enabled: touch });
}
async function fill(selector, value) {
  await evaluate(`(() => {
    const element = document.querySelector(${JSON.stringify(selector)});
    element.value = ${JSON.stringify(value)};
    element.dispatchEvent(new Event('input', { bubbles: true }));
  })()`);
}
try {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const port = server.address().port;
  for (const executable of [
    process.env.CHROME_BIN,
    "google-chrome",
    "chromium",
  ].filter(Boolean)) {
    chrome = spawn(
      executable,
      [
        "--headless=new",
        "--no-sandbox",
        "--disable-dev-shm-usage",
        "--remote-debugging-pipe",
        `--user-data-dir=${profile}`,
        "about:blank",
      ],
      { stdio: ["ignore", "ignore", "pipe", "pipe", "pipe"] },
    );
    try {
      await once(chrome, "spawn");
      break;
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
      chrome = undefined;
    }
  }
  assert(
    chrome,
    "Set CHROME_BIN to an installed Chrome or Chromium executable.",
  );
  let stderr = "";
  chrome.stderr.on("data", (data) => {
    stderr += data;
  });
  chrome.on("exit", () => {
    for (const { reject, timer } of pending.values()) {
      clearTimeout(timer);
      reject(new Error(`Browser exited: ${stderr}`));
    }
    pending.clear();
  });
  let buffer = "";
  chrome.stdio[4].on("data", (data) => {
    buffer += data;
    let end;
    while ((end = buffer.indexOf("\0")) !== -1) {
      const message = JSON.parse(buffer.slice(0, end));
      buffer = buffer.slice(end + 1);
      const request = pending.get(message.id);
      if (request) {
        pending.delete(message.id);
        clearTimeout(request.timer);
        if (message.error)
          request.reject(new Error(JSON.stringify(message.error)));
        else request.resolve(message.result);
      } else if (message.method === "Runtime.exceptionThrown") {
        errors.push(message.params.exceptionDetails);
      } else if (
        message.method === "Runtime.consoleAPICalled" &&
        message.params.type === "error"
      ) {
        errors.push(message.params.args);
      }
    }
  });
  const { targetId } = await command("Target.createTarget", {
    url: "about:blank",
  });
  ({ sessionId } = await command("Target.attachToTarget", {
    targetId,
    flatten: true,
  }));
  await command("Runtime.enable");
  await command("Page.enable");

  for (const base of [
    `http://127.0.0.1:${port}/`,
    pathToFileURL(resolve("dist/index.html")).href,
    process.env.MINICONF_WEB_DEV_URL,
  ].filter(Boolean)) {
    seed();
    writes.length = 0;
    console.log(`Connection identity and editor ownership: ${base}`);
    await viewport(1200, 850);
    await command("Page.navigate", { url: base });
    await until("document.querySelector('input[name=broker]')");
    await fill("input[name=broker]", `ws://127.0.0.1:${port}`);
    await fill("input[name=discovery-pattern]", "dt/test/+");
    await click("button[type=submit]");
    await until(
      "document.querySelector('a[data-tree-path=\"dt/test/device\"]')",
    );
    const href = await evaluate(
      "document.querySelector('a[data-tree-path=\"dt/test/device\"]').href",
    );
    await fill("input[name=broker]", "ws://different.invalid:99");
    assert.equal(
      await evaluate(
        "document.querySelector('a[data-tree-path=\"dt/test/device\"]').href",
      ),
      href,
    );
    await click('a[data-tree-path="dt/test/device"]');
    await until("document.querySelector('[data-tree-path=\"/leaf\"]')");
    await click('[data-tree-path="/leaf"]');
    await until(
      "document.querySelector('textarea')?.value === '9007199254740993'",
    );
    assert(
      await evaluate(
        "document.querySelector('[data-tree-path=\"/leaf\"]').textContent.includes(' = ')",
      ),
    );
    assert(
      await evaluate(
        "!document.body?.innerText.includes('999') && !document.querySelector('[data-tree-path=\"/obsolete\"]')",
      ),
    );
    await fill("textarea", "-9007199254740993");
    await click('[data-tree-path="/leaf"]');
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "-9007199254740993",
    );
    await click('[data-tree-path=""] button');
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "-9007199254740993",
    );
    assert(
      await evaluate(
        "document.querySelector('[data-tree-path=\"\"]').tabIndex === 0",
      ),
    );
    await evaluate(
      "document.querySelector('textarea').focus(); document.querySelector('textarea').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}))",
    );
    await until("document.activeElement.dataset.treePath === ''");
    await evaluate(
      "document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}))",
    );
    await until("document.querySelector('[data-tree-path=\"/leaf\"]')");
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "-9007199254740993",
    );
    await evaluate(
      "document.querySelector('[data-tree-path=\"/leaf\"]').dispatchEvent(new KeyboardEvent('keydown',{key:'End',bubbles:true}))",
    );
    // Keyboard navigation reveals only the target, even when the target is unchanged.
    const historyBefore = await command("Page.getNavigationHistory");
    await evaluate(
      "document.querySelector('.tree').style.height = '60px'; document.querySelector('[data-tree-path=\"\"]').focus()",
    );
    await press("End");
    await until(
      "document.activeElement.dataset.treePath === '/other' && document.querySelector('.tree').scrollTop > 0",
    );
    await evaluate("document.querySelector('.tree').scrollTop = 0");
    await press("End");
    await until("document.querySelector('.tree').scrollTop > 0");
    const treeScroll = await evaluate(
      "document.querySelector('.tree').scrollTop",
    );
    await press("Enter");
    await until("document.activeElement.matches('textarea')");
    await fill("textarea", "77");
    assert(
      !(await evaluate(
        "document.querySelector('.actions').textContent.includes('Use updated value')",
      )),
    );
    publish(`${prefix}/settings/other`, "1234");
    await until(
      "document.querySelector('[data-tree-path=\"/other\"] .value').textContent === '1234'",
    );
    assert(
      await evaluate(
        "document.querySelector('.actions').textContent.includes('Use updated value')",
      ),
    );
    assert.equal(
      await evaluate("document.querySelector('.tree').scrollTop"),
      treeScroll,
    );
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "77",
    );
    await press("Escape");
    await until("document.activeElement.dataset.treePath === '/other'");
    assert.equal(
      await evaluate("document.querySelector('.tree').scrollTop"),
      treeScroll,
    );
    await click(".schema summary");
    await click(".actions button:last-child");
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "1234",
    );
    assert(
      !(await evaluate(
        "document.querySelector('.actions').textContent.includes('Use updated value')",
      )),
    );
    assert.equal(
      await evaluate("document.querySelector('.tree').scrollTop"),
      treeScroll,
    );
    await click(".schema summary");
    assert.equal(
      (await command("Page.getNavigationHistory")).entries.length,
      historyBefore.entries.length,
    );
    await evaluate("document.querySelector('.tree').style.height = ''");
    await click(".back");
    await until("!!document.querySelector('input[name=broker]')");
    await evaluate("history.back()");
    await until(
      "document.querySelector('[data-tree-path=\"/other\"]')?.getAttribute('aria-selected') === 'true'",
    );

    // Return to the exact leaf for submission; selecting another leaf is deliberate.
    await click('[data-tree-path="/leaf"]');
    await fill("textarea", "-9007199254740993");
    holdResponse = true;
    await clickButton("Set");
    await until("document.querySelector('.actions button').disabled");
    await fill("textarea", "123");
    const before = writes.length;
    await evaluate(
      "document.querySelector('textarea').dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',ctrlKey:true,bubbles:true}))",
    );
    assert.equal(writes.length, before);
    assert.equal(
      writes.find((m) => m.topic.endsWith("/set/leaf")).payload.toString(),
      "-9007199254740993",
    );
    holdResponse = false;
    respond();
    await until("document.body?.innerText.includes('Last Set: succeeded')");
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "123",
    );
    const acceptedWrites = writes.length;
    await fill("textarea", "{");
    await clickButton("Set");
    assert(
      await evaluate(
        "document.querySelector('#editor-error').textContent.includes('Invalid JSON') && document.querySelector('.request').textContent.includes('Last Set: succeeded')",
      ),
    );
    assert.equal(writes.length, acceptedWrites);
    await fill("textarea", "123");
    assert(await evaluate("!document.querySelector('#editor-error')"));
    publish(`${prefix}/settings/leaf`, "456");
    await until(
      "document.querySelector('[data-tree-path=\"/leaf\"] .value').textContent === '456'",
    );
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "123",
    );
    await clickButton("Use updated value");
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "456",
    );
    // Returning to an old baseline must still expose the differing device value.
    await fill("textarea", "457");
    publish(`${prefix}/settings/leaf`, "458");
    await until(
      "document.querySelector('[data-tree-path=\"/leaf\"] .value')?.textContent === '458'",
    );
    await fill("textarea", "456");
    await until(
      "document.querySelector('[data-tree-path=\"/leaf\"] .value')?.textContent === '458'",
    );
    await clickButton("Use device value");
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "458",
    );

    console.log(
      `Pruning counts, captured candidates and optional permissions: ${base}`,
    );
    seed();
    writes.length = 0;
    clearAcks = 0;
    const beforePruning = connections;
    await command("Page.navigate", { url: "about:blank" });
    await command("Page.navigate", { url: href });
    await until("document.querySelector('[data-tree-path=\"/leaf\"]')");
    await click('[data-tree-path="/leaf"]');
    await until(
      "document.querySelector('.prune')?.textContent.trim() === 'Prune (3)'",
    );
    assert.equal(writes.filter((m) => m.retain).length, 0);
    assert(
      await evaluate(
        "document.querySelector('.prune').title.includes('entire device prefix')",
      ),
    );
    assert.equal(connections - beforePruning, 1);
    await viewport(390, 850);
    const headerHeight = await evaluate(
      "document.querySelector('.app-header').getBoundingClientRect().height",
    );
    holdClear = true;
    await clickButton("Prune (3)");
    assert.equal(
      await evaluate(
        "document.querySelector('.app-header').getBoundingClientRect().height",
      ),
      headerHeight,
    );
    publish(`${prefix}/settings/later`, "not JSON");
    await fill("textarea", "123");
    await clickButton("Set");
    await until("document.body?.innerText.includes('Last Set: succeeded')");
    holdClear = false;
    acknowledgeClear();
    await until("document.body?.innerText.includes('Cleared 3')");
    assert.deepEqual(
      writes
        .filter((m) => m.retain)
        .map((m) => m.topic)
        .sort(),
      ["response", "set", "settings"]
        .map((ns) => `${prefix}/${ns}/obsolete`)
        .sort(),
    );
    assert(retained.has(`${prefix}/settings/leaf`));
    await until(
      "document.querySelector('.prune')?.textContent.trim() === 'Prune (1)'",
    );
    await clickButton("Prune (1)");
    await until("!document.querySelector('.prune')");
    assert.equal(
      await evaluate(
        "document.querySelector('.app-header').getBoundingClientRect().height",
      ),
      headerHeight,
      "Prune progress and button disappearance preserve header height",
    );
    assert(await evaluate("document.body?.innerText.includes('Cleared 1')"));

    for (const name of ["a", "b", "c"])
      publish(`${prefix}/settings/${name}`, "1");
    await until(
      "document.querySelector('.prune')?.textContent.trim() === 'Prune (3)'",
    );
    rejectClearAt = clearAcks + 2;
    await clickButton("Prune (3)");
    await until("document.body?.innerText.includes('Cleared 1;')");
    assert(
      retained.has(`${prefix}/settings/b`) &&
        retained.has(`${prefix}/settings/c`),
    );
    rejectClearAt = 0;
    await clickButton("Prune (2)");
    await until("!document.querySelector('.prune')");

    // An unacknowledged clear must not be replayed by MQTT.js after reconnect.
    publish(`${prefix}/settings/interrupted`, "1");
    await until(
      "document.querySelector('.prune')?.textContent.trim() === 'Prune (1)'",
    );
    holdClear = true;
    await clickButton("Prune (1)");
    await until("!document.querySelector('.prune')");
    const clearsBeforeReconnect = writes.filter(
      (message) => message.retain,
    ).length;
    for (const socket of broker.clients) socket.terminate();
    holdClear = false;
    await until("document.body?.innerText.includes('pruning interrupted')");
    await until("!document.querySelector('.actions button').disabled");
    assert.equal(
      writes.filter((message) => message.retain).length,
      clearsBeforeReconnect,
    );

    // Activity changes its own dot, leaving selection and row geometry alone.
    const rowStyle = await evaluate(
      "getComputedStyle(document.querySelector('[data-tree-path=\"/leaf\"]')).backgroundColor",
    );
    publish(`${prefix}/settings/leaf`, "456");
    await until(
      "document.querySelector('[data-tree-path=\"/leaf\"] .activity-dot')?.style.opacity === '1'",
    );
    assert.equal(
      await evaluate(
        "getComputedStyle(document.querySelector('[data-tree-path=\"/leaf\"]')).backgroundColor",
      ),
      rowStyle,
    );
    await until(
      "document.querySelector('[data-tree-path=\"/leaf\"] .activity-dot')?.style.opacity === '0'",
    );

    rejectCleanup = true;
    await command("Page.navigate", { url: "about:blank" });
    await command("Page.navigate", { url: href });
    await until(
      "document.body?.innerText.includes('Partial pruning coverage')",
    );
    await until("document.querySelector('[data-tree-path=\"/leaf\"]')");
    publish(`${prefix}/settings/partial`, "1");
    await until(
      "document.querySelector('.prune')?.textContent.trim() === 'Prune (1)'",
    );
    await clickButton("Prune (1)");
    await until("document.body?.innerText.includes('Cleared 1')");
    assert(!retained.has(`${prefix}/settings/partial`));
    await click('[data-tree-path="/leaf"]');
    await fill("textarea", "789");
    await clickButton("Set");
    await until("document.body?.innerText.includes('Last Set: succeeded')");
    assert(
      await evaluate("!document.querySelector('.actions button').disabled"),
    );
    rejectCleanup = false;

    console.log(`Connection recovery and responsive layout: ${base}`);
    seed();
    await command("Page.navigate", { url: "about:blank" });
    await command("Page.navigate", { url: href });
    await until("document.querySelector('[data-tree-path=\"/leaf\"]')");
    await click('[data-tree-path="/leaf"]');
    // A terminal reconnect failure must offer a usable retry.
    rejectSubscriptions = true;
    for (const socket of broker.clients) socket.terminate();
    await until(
      "[...document.querySelectorAll('.connection-state button')].some(button => button.textContent.trim() === 'Retry')",
    );
    assert(
      await evaluate("document.querySelector('.actions button').disabled"),
    );
    rejectSubscriptions = false;
    await clickButton("Retry", ".connection-state");
    await until("!document.querySelector('.actions button').disabled");
    await until("!document.querySelector('.actions button').disabled");
    await fill("textarea", "789");
    // Live epoch refresh keeps the editor, replays state and resumes readiness.
    epoch += 1;
    publish(
      `${prefix}/alive`,
      JSON.stringify({ proto: 1, epoch, schema_rev: revision, pages: 1 }),
      undefined,
    );
    await until(
      "document.querySelector('.context').textContent.includes('epoch '+" +
        epoch +
        ")",
    );
    await until("!document.querySelector('.actions button').disabled");
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "789",
    );
    await until(
      "document.querySelector('[data-tree-path=\"/leaf\"] .value')?.textContent === '9007199254740993'",
    );
    // A pending Set is scoped to the current connection, including its MQTT store entry.
    holdSetAck = holdResponse = true;
    await clickButton("Set");
    await until("document.body?.innerText.includes('Setting…')");
    const writesBeforeDisconnect = writes.length;
    for (const socket of broker.clients) socket.terminate();
    await until("document.body?.innerText.includes('outcome unknown')");
    holdSetAck = holdResponse = false;
    await until("!document.querySelector('.actions button').disabled");
    assert.equal(writes.length, writesBeforeDisconnect);
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "789",
    );

    // A malformed device announcement leaves MQTT connected and the draft intact.
    const connectionsBeforeDeviceError = connections;
    publish(`${prefix}/alive`, "{}");
    await until(
      "document.querySelector('.connection-state').innerText.includes('Device unavailable')",
    );
    assert(
      await evaluate("document.querySelector('.actions button').disabled"),
    );
    assert(
      await evaluate(
        "[...document.querySelectorAll('.connection-state button')].some(button => button.textContent.trim() === 'Retry')",
      ),
    );
    publish(
      `${prefix}/alive`,
      JSON.stringify({ proto: 1, epoch, schema_rev: revision, pages: 1 }),
    );
    await until("!document.querySelector('.actions button').disabled");
    assert.equal(connections, connectionsBeforeDeviceError);
    await until(
      "document.querySelector('[data-tree-path=\"/leaf\"] .value')?.textContent === '9007199254740993'",
    );
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "789",
    );
    assert(
      await evaluate(
        "document.querySelector('.schema summary').textContent.includes('leaf · i32')",
      ),
    );
    await click('[data-tree-path=""]');
    assert(
      await evaluate(
        "!document.querySelector('.schema summary') && !!document.querySelector('.schema-body') && !document.querySelector('textarea')",
      ),
      "Internal nodes show schema without an editor or disclosure",
    );
    await click('[data-tree-path="/leaf"]');
    await click(".schema summary");
    await click('[data-tree-path="/other"]');
    await click('[data-tree-path="/leaf"]');
    assert(
      await evaluate("document.querySelector('.schema').open"),
      "Schema disclosure follows user intent across selection",
    );
    assert(
      await evaluate(`(() => {
      const text = label => document.querySelector('[aria-label="'+label+'"]').textContent;
      return !document.querySelector('.schema-hint') && document.querySelector('.schema summary h2').textContent === '/leaf' &&
        text('Edge metadata').includes('edge note') &&
        text('Node metadata').includes('first line\\nsecond line') &&
        text('Node metadata').includes('false') && text('Node metadata').includes('null') &&
        text('Semantics').includes('opaque');
    })()`),
      "Arbitrary metadata retains values and provenance without interpretation",
    );
    for (const width of [320, 390, 761, 1200]) {
      await viewport(width, 850);
      if (width === 390) {
        const draft = await evaluate(
          "document.querySelector('textarea').value",
        );
        await click('[data-tree-path=""] button');
        assert(
          await evaluate(
            "!document.querySelector('[data-tree-path=\"/leaf\"]')",
          ),
        );
        assert.equal(
          await evaluate("document.querySelector('textarea').value"),
          draft,
        );
        await click('[data-tree-path=""] button');
        await until("document.querySelector('[data-tree-path=\"/leaf\"]')");
      }
      assert(
        await evaluate(
          "document.querySelector('.app-header').getBoundingClientRect().height < 90",
        ),
        "Normal mobile identity fits a compact header",
      );
      assert(
        await evaluate(`(() => {
        const schema = document.querySelector('.schema-body');
        const editor = document.querySelector('textarea').getBoundingClientRect();
        const actions = document.querySelector('.actions').getBoundingClientRect();
        const parent = document.querySelector('.value-editor').getBoundingClientRect();
        return schema.clientHeight >= schema.scrollHeight - 1 &&
          actions.top >= editor.bottom && Math.abs(editor.width - parent.width) < 1 && editor.width > 240;
      })()`),
        "Schema is readable in the containing pane and actions leave full editor width",
      );
      assert(
        await evaluate("document.documentElement.scrollWidth <= innerWidth"),
      );
      assert(
        await evaluate(
          "document.querySelector('label[for=leaf-editor]') && document.querySelector('[role=tree][aria-label]') && document.querySelector('[role=treeitem][tabindex=\"0\"]')",
        ),
      );
      if (process.env.MINICONF_WEB_SCREENSHOTS) {
        const { data } = await command("Page.captureScreenshot", {
          format: "png",
        });
        writeFileSync(
          `.codex/browse-${width}.png`,
          Buffer.from(data, "base64"),
        );
      }
    }
    await viewport(1024, 400);
    await click(".identity summary");
    assert(
      await evaluate(
        `document.querySelector('.context h1').textContent === '${prefix}' && !document.querySelector('.identity-details').textContent.includes('${prefix}') && document.querySelectorAll('.broker-label').length === 1`,
      ),
    );
    await click(".identity summary");
    await click(".log summary");
    assert(
      await evaluate(
        "document.querySelector('.workspace').getBoundingClientRect().height >= 240",
      ),
      "Open diagnostics leave a usable workspace in short windows",
    );
    console.log(
      `Checked exact editing, broker identity, pruning, recovery and keyboard guards: ${base}`,
    );
  }
  assert.deepEqual(errors, [], "Browser console errors");
} finally {
  for (const socket of broker.clients) socket.terminate();
  broker.close();
  server.close();
  if (chrome && chrome.exitCode === null) {
    const exited = once(chrome, "exit");
    await command("Browser.close", {}, null).catch(() => chrome.kill());
    await exited;
  }
  rmSync(profile, {
    recursive: true,
    force: true,
    maxRetries: 5,
    retryDelay: 100,
  });
}
