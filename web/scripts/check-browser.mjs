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
let lastConnect;
let holdResponse = false;
let holdSetAck = false;
let respond;
let holdClear = false;
let acknowledgeClear;
let clearAcks = 0;
let rejectClearAt = 0;
let holdConnect = false;
function resetFixture() {
  epoch = 1;
  writes.length = 0;
  rejectSubscriptions = false;
  rejectCleanup = false;
  holdResponse = false;
  holdSetAck = false;
  respond = undefined;
  holdClear = false;
  acknowledgeClear = undefined;
  clearAcks = 0;
  rejectClearAt = 0;
  holdConnect = false;
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
    const reply = (value) => {
      if (socket.readyState === 1)
        socket.send(packet.generate(value, { protocolVersion: 5 }));
    };
    if (message.cmd === "connect") {
      lastConnect = message;
      const accept = () =>
        reply({
          cmd: "connack",
          reasonCode: message.username === "rejected-user" ? 0x86 : 0,
          sessionPresent: false,
        });
      if (holdConnect) broker.emit("held-connect", accept);
      else accept();
    } else if (message.cmd === "subscribe") {
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
        respond = (
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
        if (!holdResponse) respond();
      }
      broker.emit("publication", message);
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
    await command("Page.navigate", { url: "about:blank" });
    resetFixture();
    console.log(`Connection identity and editor ownership: ${base}`);
    await viewport(1200, 850);
    await command("Page.navigate", { url: base });
    await until("document.querySelector('input[name=broker]')");
    await fill("input[name=broker]", `ws://127.0.0.1:${port}`);
    await fill("input[name=discovery-pattern]", "dt/test/+");
    for (const width of [1200, 761]) {
      await viewport(width, 850);
      assert(
        await evaluate(
          `Math.abs(document.querySelector('input[name=broker]').getBoundingClientRect().top - document.querySelector('input[name=discovery-pattern]').getBoundingClientRect().top) < 1`,
        ),
        "Discovery fields align even when filter help wraps",
      );
    }
    await viewport(1200, 850);
    await click("button[type=submit]");
    await until(
      "document.querySelector('a[data-tree-path=\"/dt/test/device\"]')",
    );
    const href = await evaluate(
      "document.querySelector('a[data-tree-path=\"/dt/test/device\"]').href",
    );
    await fill("input[name=username]", "test-user");
    await fill("input[name=password]", "first-secret");
    await click("button[type=submit]");
    await until("document.querySelector('.prefixes a')");
    assert.equal(lastConnect.username, "test-user");
    assert.equal(lastConnect.password?.toString(), "first-secret");
    // Autofill can change the visible fields without delivering input events.
    await evaluate(
      "document.querySelector('input[name=username]').value = 'autofill-user'; document.querySelector('input[name=password]').value = 'replacement-secret'",
    );
    await click("button[type=submit]");
    await until("document.querySelector('.prefixes a')");
    assert.equal(lastConnect.username, "autofill-user");
    assert.equal(lastConnect.password?.toString(), "replacement-secret");
    await evaluate(
      "document.querySelector('input[name=username]').value = ''; document.querySelector('input[name=password]').value = ''",
    );
    await click("button[type=submit]");
    await until("document.querySelector('.prefixes a')");
    assert.equal(lastConnect.username ?? "", "");
    assert.equal(lastConnect.password?.toString() ?? "", "");
    await fill("input[name=username]", "active-user");
    await fill("input[name=password]", "active-secret");
    await click("button[type=submit]");
    await until("document.querySelector('.prefixes a')");
    await fill("input[name=username]", "rejected-user");
    await click("button[type=submit]");
    await until(
      "document.querySelector('.log summary')?.textContent.includes('Connection failed')",
    );
    assert.equal(
      await evaluate(
        "JSON.parse(sessionStorage.getItem('miniconf-web.connection')).username",
      ),
      "active-user",
    );
    await fill("input[name=username]", "active-user");
    await click("button[type=submit]");
    await until("document.querySelector('.prefixes a')");
    await fill("input[name=password]", "unsubmitted-secret");
    await click('[data-tree-path="/dt/test"] .toggle');
    publish("dt/test/another/alive", retained.get(`${prefix}/alive`).text);
    await until(
      "document.querySelector('.prefixes .meta').textContent === '2 found'",
    );
    assert(
      await evaluate(
        "document.querySelector('[data-tree-path=\"/dt/test\"]').getAttribute('aria-expanded') === 'false' && !document.querySelector('[data-tree-path=\"/dt/test/another\"]')",
      ),
    );
    await click('[data-tree-path="/dt/test"] .toggle');
    await until(
      "document.querySelector('[data-tree-path=\"/dt/test/another\"]')",
    );
    publish("dt/test/another/alive", "");
    await until(
      "document.querySelector('.prefixes .meta').textContent === '1 found'",
    );
    await fill("input[name=broker]", "ws://different.invalid:99");
    assert.equal(
      await evaluate(
        "document.querySelector('a[data-tree-path=\"/dt/test/device\"]').href",
      ),
      href,
    );
    await click('a[data-tree-path="/dt/test/device"]');
    await until("document.querySelector('[data-tree-path=\"/leaf\"]')");
    assert.equal(lastConnect.username, "active-user");
    assert.equal(lastConnect.password?.toString(), "active-secret");
    assert(
      await evaluate(
        "!location.href.includes('active-secret') && !JSON.stringify(history.state).includes('active-secret') && !JSON.stringify(localStorage).includes('active-secret') && JSON.parse(sessionStorage.getItem('miniconf-web.connection')).password === 'active-secret'",
      ),
    );
    await click('[data-tree-path="/leaf"]');
    await until(
      "document.querySelector('textarea')?.value === '9007199254740993'",
    );
    assert(
      await evaluate(`(() => {
      const row = document.querySelector('[data-tree-path="/leaf"]');
      return row.querySelector('.summary').textContent === ' (i32)' &&
        row.title.includes('Edge metadata:') && row.title.includes('Node metadata:') &&
        row.title.includes('Signed digital mixer step') && !row.title.includes('Arrows/');
    })()`),
      "Tree rows show semantics and useful schema tooltips",
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
    assert.equal(
      await evaluate(
        "document.querySelector('.identity-details').textContent.trim()",
      ),
      "2 settings",
    );
    await click(".log summary");
    assert(await evaluate("!document.querySelector('.log pre')"));
    assert(
      await evaluate(`(() => {
        const body = document.querySelector('.log-body');
        return Math.abs(body.clientHeight - 10 * parseFloat(getComputedStyle(body).lineHeight)) < 1;
      })()`),
      "Open log reserves ten text lines, including when empty",
    );
    publish(`${prefix}/settings/leaf`, "9007199254740993");
    publish(`${prefix}/settings/leaf`, "9007199254740993");
    await until(
      "document.querySelector('.log pre')?.textContent.includes('settings: 0 changed · 1 observed')",
    );
    publish(`${prefix}/settings/leaf`, "20");
    publish(`${prefix}/settings/leaf`, "20.0");
    await until(
      "document.querySelector('.log pre')?.textContent.includes('settings: 1 changed · 1 observed') && document.querySelector('textarea').value === '20.0'",
    );
    publish(`${prefix}/settings/leaf`, "9007199254740993");
    await until(
      "document.querySelector('textarea').value === '9007199254740993'",
    );
    await click(".log summary");
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
        "document.querySelector('.actions button:last-child').disabled",
      )),
    );
    publish(`${prefix}/settings/other`, "1234");
    await until(
      "document.querySelector('[data-tree-path=\"/other\"] .value').textContent === '1234'",
    );
    assert(
      await evaluate(
        "!document.querySelector('.actions button:last-child').disabled",
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
    await click(".actions button:last-child");
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "1234",
    );
    assert(
      await evaluate(
        "document.querySelector('.actions button:last-child').disabled",
      ),
    );
    assert.equal(
      await evaluate("document.querySelector('.tree').scrollTop"),
      treeScroll,
    );
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
    await click(".log summary");
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
    await until(
      "document.querySelector('.status [role=status]')?.textContent.trim() === 'Set succeeded'",
    );
    await until(
      "document.querySelector('textarea').value === '-9007199254740993'",
    );
    assert(
      await evaluate(
        "document.querySelector('.status').textContent.trim() === 'Set succeeded'",
      ),
    );
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "-9007199254740993",
    );
    assert.match(
      await evaluate("document.querySelector('.log pre').textContent"),
      /set: \/leaf: Set succeeded · \d+ ms/,
    );
    await click(".log summary");
    const acceptedWrites = writes.length;
    await fill("textarea", "{");
    await clickButton("Set");
    assert(
      await evaluate(
        "document.querySelector('#editor-error').textContent.includes('Invalid JSON') && document.querySelector('.status').textContent.trim() === 'Ready' && !document.querySelector('.selected [role=status]')",
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
    await clickButton("Revert");
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
    await clickButton("Revert");
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "458",
    );

    // Matching an unsolicited value disables Revert but does not release a draft.
    await fill("textarea", "3");
    publish(`${prefix}/settings/leaf`, "3");
    await until(
      "document.querySelector('[data-tree-path=\"/leaf\"] .value')?.textContent === '3' && document.querySelector('.actions button:last-child').disabled",
    );
    publish(`${prefix}/settings/leaf`, "4");
    await until(
      "document.querySelector('[data-tree-path=\"/leaf\"] .value')?.textContent === '4'",
    );
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "3",
    );
    assert(
      await evaluate(
        "!document.querySelector('.actions button:last-child').disabled",
      ),
    );

    // Device formatting wins on success, with either publication/reply arrival order.
    for (const [input, value, echoFirst] of [
      ["20", "20.0", true],
      ["30", "30.0", false],
    ]) {
      holdResponse = true;
      await fill("textarea", input);
      await clickButton("Set");
      await until("document.querySelector('.actions button').disabled");
      respond(value, echoFirst);
      await until(
        `document.querySelector('textarea').value === ${JSON.stringify(value)} && document.querySelector('.actions button:last-child').disabled`,
      );
      holdResponse = false;
    }

    // Action errors remain readable on mobile and leave the rejected draft intact.
    await viewport(390, 850);
    holdResponse = true;
    await fill("textarea", "999");
    await clickButton("Set");
    await until("document.querySelector('.actions button').disabled");
    const rejection =
      "Value exceeds the supported range for this setting. Choose a smaller value and try again.";
    respond(rejection, true, "Error");
    await until("document.querySelector('.status [role=status].failed')");
    assert.equal(
      await evaluate(
        "document.querySelector('.status [role=status]').textContent.trim()",
      ),
      `Set failed: ${rejection}`,
    );
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "999",
    );
    assert(
      await evaluate(`(() => {
      const status = document.querySelector('.status [role=status]');
      return status.scrollWidth <= status.clientWidth && status.scrollHeight <= status.clientHeight &&
        status.clientHeight > parseFloat(getComputedStyle(status).lineHeight);
    })()`),
    );
    publish(`${prefix}/settings/leaf`, "31.0");
    await until(
      "document.querySelector('[data-tree-path=\"/leaf\"] .value').textContent === '31.0'",
    );
    assert.equal(
      await evaluate(
        "document.querySelector('.status [role=status]').textContent.trim()",
      ),
      `Set failed: ${rejection}`,
    );
    holdResponse = false;
    await clickButton("Set");
    await until(
      "document.querySelector('.status [role=status]')?.textContent.trim() === 'Set succeeded'",
    );
    assert(await evaluate("!document.querySelector('.status .failed')"));

    console.log(
      `Device loss, reboot, schema replacement and tab reload: ${base}`,
    );
    await command("Page.navigate", { url: "about:blank" });
    resetFixture();
    await viewport(1200, 850);
    await command("Page.navigate", { url: href });
    await until(
      "document.querySelector('[data-tree-path=\"/leaf\"] .value')?.textContent === '9007199254740993'",
    );
    await click('[data-tree-path="/leaf"]');
    await fill("textarea", "");
    publish(`${prefix}/alive`, "");
    await until(
      "document.querySelector('.status').textContent.includes('Waiting for device') && document.querySelector('.actions button').disabled",
    );
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "",
    );
    assert(
      await evaluate(
        "!document.querySelector('.actions button:last-child').disabled",
      ),
    );
    // Startup publications may precede the new alive commit marker.
    publish(`${prefix}/settings/leaf`, "2");
    publish(
      `${prefix}/alive`,
      JSON.stringify({
        proto: 1,
        epoch: ++epoch,
        schema_rev: revision,
        pages: 1,
      }),
    );
    await until(
      "document.querySelector('[data-tree-path=\"/leaf\"] .value')?.textContent === '2' && !document.querySelector('.actions button').disabled",
    );
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "",
    );
    await clickButton("Revert");
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "2",
    );
    await until(
      "document.querySelector('.prune')?.textContent.trim() === 'Prune (3)'",
    );
    await fill("textarea", "99");
    publish(`${prefix}/alive`, "");
    await until(
      "document.querySelector('.status').textContent.includes('Waiting for device')",
    );
    const nextSchema = schema.replace('"leaf":', '"replacement":');
    let nextRevision = 0x811c9dc5;
    for (const byte of new TextEncoder().encode(nextSchema))
      nextRevision = Math.imul(nextRevision ^ byte, 0x01000193) >>> 0;
    publish(`${prefix}/schema/0`, nextSchema);
    publish(`${prefix}/settings/replacement`, "3");
    publish(
      `${prefix}/alive`,
      JSON.stringify({ proto: 1, epoch, schema_rev: nextRevision, pages: 1 }),
    );
    await until(
      "document.querySelector('[data-tree-path=\"/replacement\"] .value')?.textContent === '3'",
    );
    assert.equal(lastConnect.username, "active-user");
    assert.equal(lastConnect.password?.toString(), "active-secret");
    assert(
      await evaluate(
        "!document.querySelector('[data-tree-path=\"/leaf\"]') && document.querySelector('.selected').textContent.includes('Leaf unavailable') && document.querySelector('.actions button').disabled",
      ),
    );
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "99",
    );
    await until(
      "document.querySelector('.prune')?.textContent.trim() === 'Prune (4)'",
    );
    const previousDocument = await evaluate("performance.timeOrigin");
    await command("Page.reload");
    await until(`performance.timeOrigin !== ${previousDocument}`);
    await until(
      "document.querySelector('[data-tree-path=\"/replacement\"] .value')?.textContent === '3'",
    );
    assert.equal(await evaluate("location.href"), href);
    assert(await evaluate("!document.querySelector('textarea')"));
    await click('[data-tree-path="/replacement"]');
    assert.equal(
      await evaluate("document.querySelector('textarea').value"),
      "3",
    );
    assert.equal(
      writes.length,
      0,
      "Lifecycle recovery and reload never send edits or prune automatically",
    );

    console.log(
      `Pruning counts, captured candidates and optional permissions: ${base}`,
    );
    const beforePruning = connections;
    await command("Page.navigate", { url: "about:blank" });
    resetFixture();
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
    holdResponse = true;
    await clickButton("Set");
    await until("document.querySelector('.actions button').disabled");
    respond("Rejected during pruning", true, "Error");
    holdResponse = false;
    await until("!document.querySelector('.actions button').disabled");
    assert.equal(
      await evaluate(
        "document.querySelector('.status [role=status]').textContent.trim()",
      ),
      "Set failed: Rejected during pruning",
    );
    holdClear = false;
    acknowledgeClear();
    await until(
      "document.querySelector('.prune') && !document.querySelector('.prune').disabled",
    );
    assert.equal(
      await evaluate(
        "document.querySelector('.status [role=status]').textContent.trim()",
      ),
      "Set failed: Rejected during pruning",
    );
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
    await until("document.body?.innerText.includes('Set succeeded')");
    assert(
      await evaluate("!document.querySelector('.actions button').disabled"),
    );
    rejectCleanup = false;

    console.log(`Connection recovery and responsive layout: ${base}`);
    await command("Page.navigate", { url: "about:blank" });
    resetFixture();
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
    await fill("textarea", "789");
    // Live epoch refresh keeps the editor, replays state and resumes readiness.
    epoch += 1;
    publish(
      `${prefix}/alive`,
      JSON.stringify({ proto: 1, epoch, schema_rev: revision, pages: 1 }),
      undefined,
    );
    await until(
      "document.querySelector('.context h1').title.includes('Epoch '+" +
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
        "!document.querySelector('.selected details') && document.querySelector('.schema-body').textContent.includes('i32')",
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
    await click('[data-tree-path="/other"]');
    await click('[data-tree-path="/leaf"]');
    assert(
      await evaluate(
        "!!document.querySelector('.schema-body') && !document.querySelector('.selected details')",
      ),
      "Schema stays visible across selection",
    );
    assert(
      await evaluate(`(() => {
      const text = label => document.querySelector('[aria-label="'+label+'"]').textContent;
      return document.querySelector('.selected h2').textContent === '/leaf' &&
        text('Edge metadata').includes('edge note') &&
        text('Node metadata').includes('first line\\nsecond line') &&
        text('Node metadata').includes('false') && text('Node metadata').includes('null') &&
        text('Semantics').includes('opaque');
    })()`),
      "Arbitrary metadata retains values and provenance without interpretation",
    );
    for (const width of [320, 390, 761, 1200]) {
      await viewport(width, 850);
      assert(
        await evaluate(`(() => {
        const broker = document.querySelector('.back').getBoundingClientRect();
        const prefix = document.querySelector('.context h1').getBoundingClientRect();
        const status = document.querySelector('.status').getBoundingClientRect();
        return broker.width > 0 && prefix.width > 0 && status.width > 0 &&
          [document.querySelector('.back'), document.querySelector('.context h1')].every(element => element.scrollWidth <= element.clientWidth + 1 && getComputedStyle(element).textOverflow !== 'ellipsis');
      })()`),
        "Header identities remain fully readable at every width",
      );
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
        await evaluate("!document.querySelector('.app-header details')"),
        "Header details need no disclosure",
      );
      assert(
        await evaluate(`(() => {
        const schema = document.querySelector('.schema-body');
        const editor = document.querySelector('textarea').getBoundingClientRect();
        const actions = document.querySelector('.actions').getBoundingClientRect();
        const parent = document.querySelector('.value-editor').getBoundingClientRect();
        return schema.getBoundingClientRect().top >= actions.bottom && schema.clientHeight >= schema.scrollHeight - 1 &&
          actions.top >= editor.bottom && Math.abs(editor.width - parent.width) < 1 && editor.width > 240;
      })()`),
        `Schema follows the full-width editor at ${width}px: ${JSON.stringify(await evaluate(`['.schema-body', 'textarea', '.actions', '.value-editor'].map(selector => { const element = document.querySelector(selector); return { selector, rect: element.getBoundingClientRect().toJSON(), height: element.clientHeight, scroll: element.scrollHeight }; })`))}`,
      );
      assert(
        await evaluate("document.documentElement.scrollWidth <= innerWidth"),
      );
      assert(
        await evaluate(
          "document.querySelector('textarea[aria-label]') && !document.querySelector('label[for=leaf-editor]') && document.querySelector('[role=tree][aria-label]') && document.querySelector('[role=treeitem][tabindex=\"0\"]')",
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
    assert(
      await evaluate(
        `document.querySelector('.context h1').textContent === '${prefix}' && document.querySelector('.back').textContent === 'ws://127.0.0.1:${port}' && document.querySelector('.identity-details').textContent.trim() === '2 settings' && document.querySelector('.context h1').title.includes('${revision}')`,
      ),
    );
    await click(".log summary");
    assert(
      await evaluate(
        "document.querySelector('.workspace').getBoundingClientRect().height >= 240",
      ),
      "Open diagnostics leave a usable workspace in short windows",
    );

    console.log(`Same-document route cancellation: ${base}`);
    const timeOrigin = await evaluate("performance.timeOrigin");
    await evaluate("location.hash = ''");
    await until("document.querySelector('input[name=broker]')");
    resetFixture();
    holdConnect = true;
    const connecting = once(broker, "held-connect", {
      signal: AbortSignal.timeout(5000),
    });
    await evaluate(`location.hash = ${JSON.stringify(new URL(href).hash)}`);
    const [accept] = await connecting;
    await evaluate("location.hash = ''");
    await until(
      "document.querySelector('.log summary')?.textContent.includes('Not connected')",
    );
    holdConnect = false;
    accept();

    await evaluate(`location.hash = ${JSON.stringify(new URL(href).hash)}`);
    await until("document.querySelector('[data-tree-path=\"/leaf\"] .value')");
    await click('[data-tree-path="/leaf"]');
    holdResponse = holdSetAck = holdClear = true;
    await fill("textarea", "42");
    const setting = once(broker, "publication", {
      signal: AbortSignal.timeout(5000),
    });
    await clickButton("Set");
    await setting;
    const pruning = once(broker, "publication", {
      signal: AbortSignal.timeout(5000),
    });
    await clickButton("Prune (3)");
    await pruning;
    const lateResponse = respond;
    const lateClear = acknowledgeClear;
    const deviceHash = new URL(href).hash;
    const subtreeHash = `${deviceHash}${deviceHash.includes("?") ? "&" : "?"}path=%2Fother`;
    await evaluate(`location.hash = ${JSON.stringify(subtreeHash)}`);
    await until(
      "document.querySelector('.selected h2')?.textContent === '/other'",
    );
    lateResponse();
    lateClear();
    holdResponse = holdSetAck = holdClear = false;
    await until(
      "document.querySelector('.status [role=status]')?.textContent.trim() === 'Ready'",
    );
    await until("document.querySelector('textarea')?.value === 'null'");
    assert(
      await evaluate("!document.querySelector('.actions button').disabled"),
    );
    assert.equal(await evaluate("performance.timeOrigin"), timeOrigin);
    await until(
      "document.querySelector('.identity-details')?.textContent.trim() === '1 setting in subtree' && document.querySelector('.selected h2').textContent === '/other'",
    );
    await until(
      "document.querySelector('textarea')?.value === 'null' && document.querySelector('.log pre')?.textContent.split('\\n')[0].includes('settings: 0 changed · 1 observed')",
    );
    console.log(
      `Checked exact editing, broker identity, pruning, recovery and keyboard guards: ${base}`,
    );
    // Missing tab credentials must prompt without silently connecting anonymously.
    await click(".back");
    await until("document.querySelector('input[name=username]')");
    await fill("input[name=username]", "reload-user");
    await fill("input[name=password]", "reload-secret");
    await click("button[type=submit]");
    await until("document.querySelector('.prefixes a')");
    await evaluate(`location.hash = ${JSON.stringify(subtreeHash)}`);
    await until(
      "document.querySelector('.selected h2')?.textContent === '/other' && document.querySelector('textarea')?.value === 'null'",
    );
    const resumeUrl = await evaluate("location.href");
    const resumeOrigin = await evaluate("performance.timeOrigin");
    const beforePrompt = connections;
    await evaluate("sessionStorage.removeItem('miniconf-web.connection')");
    await command("Page.reload");
    await until(`performance.timeOrigin !== ${resumeOrigin}`);
    await until(
      "document.querySelector('.log summary')?.textContent.includes('Enter credentials to reconnect')",
    );
    assert.equal(connections, beforePrompt);
    assert.equal(await evaluate("location.href"), resumeUrl);
    await fill("input[name=username]", "resume-user");
    await fill("input[name=password]", "resume-secret");
    await evaluate(
      "window.restoreStorageSet = Storage.prototype.setItem; Storage.prototype.setItem = () => { throw new DOMException('Storage denied', 'SecurityError'); }",
    );
    await click("button[type=submit]");
    await until(
      "document.querySelector('.selected h2')?.textContent === '/other' && document.querySelector('textarea')?.value === 'null'",
    );
    assert.equal(lastConnect.username, "resume-user");
    assert.equal(lastConnect.password?.toString(), "resume-secret");
    await until(
      "document.querySelector('.status')?.textContent.includes('storage unavailable')",
    );
    await evaluate(
      "Storage.prototype.setItem = window.restoreStorageSet; delete window.restoreStorageSet",
    );
    assert.equal(await evaluate("location.href"), resumeUrl);
    await evaluate("location.hash = ''");
    await until("document.querySelector('input[name=broker]')");
    assert(
      await evaluate(
        "sessionStorage.getItem('miniconf-web.connection') === null",
      ),
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
