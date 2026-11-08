import { afterEach, describe, expect, it, vi } from "vitest";
import { CUES, playCue, setAudioPrefs } from "./audio";

afterEach(() => setAudioPrefs({ volume: 0.8, muted: false }));

describe("playCue", () => {
  it("plays the bundled cue at the configured volume", () => {
    const play = vi.fn().mockResolvedValue(undefined);
    const created: { src: string; volume: number }[] = [];
    setAudioPrefs({ volume: 0.5 });
    playCue("error", (src) => {
      const a = { src, volume: 1, play };
      created.push(a);
      return a;
    });
    expect(created[0]?.src).toBe(CUES.error);
    expect(created[0]?.volume).toBe(0.5);
    expect(play).toHaveBeenCalledOnce();
  });

  it("is silent when muted and never throws when playback is refused", () => {
    const create = vi.fn(() => ({
      volume: 1,
      play: vi.fn().mockRejectedValue(new Error("autoplay blocked")),
    }));
    setAudioPrefs({ muted: true });
    playCue("tick", create);
    expect(create).not.toHaveBeenCalled();
    setAudioPrefs({ muted: false });
    expect(() => playCue("tick", create)).not.toThrow();
  });

  it("tolerates engines where play() returns undefined or throws", () => {
    expect(() =>
      playCue("tick", () => ({ volume: 1, play: () => undefined as unknown as Promise<void> })),
    ).not.toThrow();
    expect(() =>
      playCue("tick", () => {
        throw new Error("no audio device");
      }),
    ).not.toThrow();
  });

  it("has a cue for every till event", () => {
    expect(Object.keys(CUES).sort()).toEqual(["approval", "complete", "error", "tick", "warning"]);
  });
});
