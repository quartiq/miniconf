import { describe, expect, it, vi } from "vitest";
import { SettingsMirror, type SettingsCommit } from "./settings-mirror";

describe("SettingsMirror", () => {
  it("coalesces settings updates by timer", () => {
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
      expect([...commits[0].activity]).toEqual([]);
      expect(commits[0].rev).toBe("13");

      mirror.ingest("/a", 3, true);
      vi.advanceTimersByTime(100);
      expect([...commits[1].activity]).toEqual(["/a"]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("deletes only exact leaves published absent", () => {
    vi.useFakeTimers();
    try {
      const commits: SettingsCommit[] = [];
      const mirror = new SettingsMirror((commit) => commits.push(commit));

      mirror.ingest("/a", 1, true);
      mirror.ingest("/b", 2, true);
      vi.runAllTimers();

      mirror.ingest("/a", undefined, false);
      vi.runAllTimers();

      expect([...commits.at(-1)!.settings]).toEqual([["/b", 2]]);
      expect([...commits.at(-1)!.changed]).toEqual(["/a"]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("clears current settings explicitly on reload", () => {
    vi.useFakeTimers();
    try {
      const commits: SettingsCommit[] = [];
      const mirror = new SettingsMirror((commit) => commits.push(commit));

      mirror.ingest("/a", 1, true);
      mirror.ingest("/b", 2, true);
      vi.runAllTimers();

      mirror.clear();

      expect([...commits.at(-1)!.settings]).toEqual([]);
      expect([...commits.at(-1)!.changed].sort()).toEqual(["/a", "/b"]);
      expect([...commits.at(-1)!.activity]).toEqual([]);

      mirror.ingest("/a", 3, true);
      vi.runAllTimers();
      expect([...commits.at(-1)!.activity]).toEqual([]);
    } finally {
      vi.useRealTimers();
    }
  });
});
