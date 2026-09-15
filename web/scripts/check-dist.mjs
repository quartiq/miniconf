import { readdir, readFile } from "node:fs/promises";

const files = await readdir("dist", { recursive: true });
if (files.length !== 1 || files[0] !== "index.html") {
  throw new Error(`Expected only dist/index.html, found: ${files.join(", ")}`);
}

const html = await readFile("dist/index.html", "utf8");
if (!html.includes("<script") || !html.includes("<style")) {
  throw new Error(
    "dist/index.html does not contain inlined scripts and styles",
  );
}
if (/<script\b[^>]*\bsrc=|<link\b[^>]*\brel=["']stylesheet/i.test(html)) {
  throw new Error("dist/index.html still references an external asset");
}
