import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import BrowseView from "./BrowseView.svelte";

describe("BrowseView", () => {
  it("renders the browse shell and dirty editor state", () => {
    const rendered = render(BrowseView, {
      props: {
        broker: "wss://broker.example/settings",
        activePrefix: "dt/device",
        discoverHref: "#/discover",
        subtreePath: "",
        aliveManifest: { epoch: 1, schema_rev: 2 },
        settingsRevision: "3",
        status: "Error",
        error: "Connection failed",
        retryable: true,
        treeNodes: new Map(),
        selectedPath: "/leaf",
        selected: { path: "/leaf", kind: "leaf", children: [], present: true, value: 1 },
        activity: new Map(),
        expanded: new Set(),
        treeRoot: "",
        editor: "2",
        editorDirty: true,
        editorStale: false,
        logLines: [],
        treeActions: {
          activate: () => {},
          key: () => "",
          open: () => {},
          select: () => {},
        },
        updateEditor: () => {},
        submit: () => {},
        resetEditor: () => {},
        focusTree: () => {},
        retry: () => {},
      },
    });

    expect(rendered.body).toContain("Retry");
    expect(rendered.body).toContain("Edited");
  });
});
