import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsMirror, type SettingsCommit } from "./settings-mirror";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("SettingsMirror", () => {
  it("coalesces settings updates by timer", () => {
    const commits: SettingsCommit[] = [];
    const mirror = new SettingsMirror((commit) => commits.push(commit), 100);

    mirror.ingest("/a", "1");
    mirror.ingest("/a", "2", "13");
    expect(commits).toHaveLength(0);

    vi.advanceTimersByTime(100);
    expect(commits).toHaveLength(1);
    expect(commits[0].settings.get("/a")).toBe("2");
    expect([...commits[0].touched]).toEqual(["/a"]);
    expect([...commits[0].activity]).toEqual(["/a"]);
    expect(commits[0].rev).toBe("13");

    mirror.ingest("/a", "3");
    mirror.ingest("/b", "4");
    vi.advanceTimersByTime(100);
    expect([...commits[1].activity]).toEqual(["/a", "/b"]);
    mirror.clear();
    expect([...commits[0].settings]).toEqual([["/a", "2"]]);
    expect([...commits[0].touched]).toEqual(["/a"]);
    expect([...commits[1].touched]).toEqual(["/a", "/b"]);
  });

  it("deletes only exact leaves published absent", () => {
    const commits: SettingsCommit[] = [];
    const mirror = new SettingsMirror((commit) => commits.push(commit));

    mirror.ingest("/a", "1");
    mirror.ingest("/b", "2");
    vi.runAllTimers();

    mirror.ingest("/a", undefined);
    vi.runAllTimers();

    expect([...commits.at(-1)!.settings]).toEqual([["/b", "2"]]);
    expect([...commits.at(-1)!.touched]).toEqual(["/a"]);
  });

  it("clears current settings explicitly on reload", () => {
    const commits: SettingsCommit[] = [];
    const mirror = new SettingsMirror((commit) => commits.push(commit));

    mirror.ingest("/a", "1");
    mirror.ingest("/b", "2");
    vi.runAllTimers();

    mirror.clear();

    expect([...commits.at(-1)!.settings]).toEqual([]);
    expect([...commits.at(-1)!.touched].sort()).toEqual(["/a", "/b"]);
    expect([...commits.at(-1)!.activity]).toEqual([]);

    mirror.ingest("/a", "3");
    vi.runAllTimers();
    expect([...commits.at(-1)!.activity]).toEqual(["/a"]);
  });
});
