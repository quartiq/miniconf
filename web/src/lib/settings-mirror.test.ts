import { describe, expect, it, vi } from "vitest";
import { SettingsMirror, type SettingsCommit } from "./settings-mirror";

describe("SettingsMirror", () => {
  it("buffers retained updates until retained completion", () => {
    const commits: SettingsCommit[] = [];
    const mirror = new SettingsMirror((commit) => commits.push(commit));

    mirror.beginRetained();
    mirror.ingest("/a", 1, true);
    mirror.ingest("/b", 2, true, "12");
    expect(commits).toHaveLength(0);

    mirror.finishRetained(new Map([["/a", 3]]));
    expect(commits).toHaveLength(1);
    expect([...commits[0].settings]).toEqual([
      ["/a", 3],
      ["/b", 2],
    ]);
    expect([...commits[0].changed].sort()).toEqual(["/a", "/b"]);
    expect(commits[0].rev).toBe("12");
    expect(commits[0].source).toBe("retained");
  });

  it("marks retained entries absent after a retained refresh", () => {
    vi.useFakeTimers();
    try {
      const commits: SettingsCommit[] = [];
      const mirror = new SettingsMirror((commit) => commits.push(commit));

      mirror.ingest("/a", 1, true);
      mirror.ingest("/b", 2, true);
      vi.runAllTimers();

      mirror.beginRetained();
      mirror.finishRetained(new Map([["/a", 3]]));

      expect([...commits.at(-1)!.settings]).toEqual([["/a", 3]]);
      expect([...commits.at(-1)!.changed].sort()).toEqual(["/a", "/b"]);
      expect(commits.at(-1)!.source).toBe("retained");
    } finally {
      vi.useRealTimers();
    }
  });

  it("preserves current settings when retained refresh fails", () => {
    vi.useFakeTimers();
    try {
      const commits: SettingsCommit[] = [];
      const mirror = new SettingsMirror((commit) => commits.push(commit));

      mirror.ingest("/a", 1, true);
      vi.runAllTimers();

      mirror.beginRetained();
      mirror.ingest("/a", 2, true);
      mirror.failRetained();

      expect(commits).toHaveLength(1);
      expect([...commits[0].settings]).toEqual([["/a", 1]]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("coalesces live updates by timer", () => {
    vi.useFakeTimers();
    try {
      const commits: SettingsCommit[] = [];
      const mirror = new SettingsMirror((commit) => commits.push(commit), 100);

      mirror.ingest("/a", 1, true);
      mirror.ingest("/a", 2, true, "13");
      expect(commits).toHaveLength(0);

      vi.advanceTimersByTime(100);
      expect(commits).toHaveLength(1);
      expect(commits[0].settings.get("/a")).toBe(2);
      expect([...commits[0].changed]).toEqual(["/a"]);
      expect(commits[0].rev).toBe("13");
      expect(commits[0].source).toBe("live");
    } finally {
      vi.useRealTimers();
    }
  });
});
