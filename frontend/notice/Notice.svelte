<script lang="ts">
  // The notice a scheduled Game Saves backup leaves at the bottom right of the screen: what happened, a short chime, and a click to open MYLE. It goes by itself after a few seconds, and waits while the pointer is on it.
  import { onMount } from "svelte";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import CheckCheck from "@lucide/svelte/icons/check-check";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import X from "@lucide/svelte/icons/x";

  interface NoticeData {
    kind: "backedUp" | "nothingNew" | "failed";
    games: number;
    failed: string[];
    error: string | null;
    at: number;
  }

  /** How long it stays; longer when something went wrong. */
  const STAY_MS = { backedUp: 7000, nothingNew: 6000, failed: 12000 };

  let notice = $state<NoticeData | null>(null);
  let leaving = $state(false);
  let paused = $state(false);
  let left = $state(1);

  const plural = (count: number, one: string, many: string) => `${count} ${count === 1 ? one : many}`;

  const view = $derived.by(() => {
    if (!notice) return null;
    if (notice.kind === "backedUp") {
      return {
        tone: "ok",
        icon: ArchiveRestore,
        title: "Game saves backed up",
        text: `${plural(notice.games, "game", "games")} copied to your backup folder.`,
      };
    }
    if (notice.kind === "nothingNew") {
      return {
        tone: "calm",
        icon: CheckCheck,
        title: "Your game saves are up to date",
        text: "Nothing new or changed since the last backup.",
      };
    }
    const some = notice.failed.length
      ? `${plural(notice.failed.length, "game", "games")} could not be backed up: ${notice.failed.join(", ")}.`
      : null;
    return {
      tone: "bad",
      icon: TriangleAlert,
      title: notice.games ? `Backed up ${plural(notice.games, "game", "games")}, not all` : "Automatic backup failed",
      text: some ?? notice.error ?? "The backup could not run.",
    };
  });

  const time = $derived(
    notice ? new Intl.DateTimeFormat(undefined, { timeStyle: "short" }).format(new Date(notice.at * 1000)) : "",
  );

  /** Two soft notes, rising when all went well and falling when not. */
  function chime(kind: NoticeData["kind"]) {
    try {
      const audio = new AudioContext();
      const notes = kind === "failed" ? [659.25, 493.88] : [783.99, 1174.66];
      const start = audio.currentTime + 0.05;
      notes.forEach((frequency, index) => {
        const at = start + index * 0.13;
        for (const [type, ratio, volume] of [
          ["sine", 1, 0.11],
          ["triangle", 2, 0.025],
        ] as const) {
          const tone = audio.createOscillator();
          const gain = audio.createGain();
          tone.type = type;
          tone.frequency.value = frequency * ratio;
          gain.gain.setValueAtTime(0, at);
          gain.gain.linearRampToValueAtTime(volume, at + 0.012);
          gain.gain.exponentialRampToValueAtTime(0.0001, at + 0.75);
          tone.connect(gain).connect(audio.destination);
          tone.start(at);
          tone.stop(at + 0.8);
        }
      });
      setTimeout(() => void audio.close(), 1500);
    } catch {
      // No sound device: the notice is still there to read.
    }
  }

  async function close(open: boolean) {
    if (leaving) return;
    leaving = true;
    await new Promise((resolve) => setTimeout(resolve, 220));
    if (isTauri()) await invoke("notice_done", { open }).catch(() => window.close());
    else leaving = false;
  }

  onMount(() => {
    let frame = 0;
    void (async () => {
      notice = isTauri()
        ? await invoke<NoticeData>("notice_data")
        : {
            kind: (new URLSearchParams(location.search).get("kind") as NoticeData["kind"]) ?? "backedUp",
            games: 3,
            failed: new URLSearchParams(location.search).get("kind") === "failed" ? ["Hades II"] : [],
            error: null,
            at: Date.now() / 1000,
          };
      chime(notice.kind);
      // The time left, paused while the pointer is on the card.
      const total = STAY_MS[notice.kind];
      let remaining = total;
      let last = performance.now();
      const tick = (now: number) => {
        if (!paused) remaining -= now - last;
        last = now;
        left = Math.max(0, remaining / total);
        if (remaining <= 0) void close(false);
        else frame = requestAnimationFrame(tick);
      };
      frame = requestAnimationFrame(tick);
    })();
    return () => cancelAnimationFrame(frame);
  });
</script>

{#if view}
  <div
    class="card {view.tone}"
    class:leaving
    role="button"
    tabindex="-1"
    title="Open Game Saves"
    onclick={() => close(true)}
    onkeydown={(event) => event.key === "Enter" && close(true)}
    onpointerenter={() => (paused = true)}
    onpointerleave={() => (paused = false)}
  >
    <span class="icon"><view.icon size={20} strokeWidth={1.9} /></span>
    <div class="body">
      <div class="head">
        <span class="app">MYLE · Game Saves</span>
        <span class="time">{time}</span>
      </div>
      <strong>{view.title}</strong>
      <p>{view.text}</p>
    </div>
    <button
      class="close"
      aria-label="Close"
      onclick={(event) => {
        event.stopPropagation();
        void close(false);
      }}><X size={14} /></button
    >
    <span class="timer" style:transform="scaleX({left})" aria-hidden="true"></span>
  </div>
{/if}

<style>
  :global(body) {
    padding: 10px;
    overflow: hidden;
    user-select: none;
  }

  .card {
    --tone: var(--accent-rgb);
    position: relative;
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: start;
    gap: 12px;
    height: 92px;
    padding: 13px 12px 13px 14px;
    overflow: hidden;
    border: 1px solid rgb(255 255 255 / 0.09);
    border-radius: 14px;
    background:
      radial-gradient(120% 140% at 0% 0%, rgb(var(--tone) / 0.16), transparent 55%),
      linear-gradient(180deg, rgb(26 31 46 / 0.97), rgb(15 18 28 / 0.97));
    box-shadow:
      0 12px 32px -8px rgb(0 0 0 / 0.65),
      0 2px 6px rgb(0 0 0 / 0.35),
      inset 0 1px 0 rgb(255 255 255 / 0.07);
    color: var(--text-1);
    font-family: var(--font-sans);
    cursor: pointer;
    animation: enter 420ms cubic-bezier(0.2, 0.9, 0.25, 1.1) both;
    transition:
      transform 220ms var(--ease-out),
      opacity 220ms var(--ease-out),
      border-color 160ms var(--ease-out);
  }

  .card:hover {
    border-color: rgb(var(--tone) / 0.35);
  }

  .card.leaving {
    transform: translateX(28px);
    opacity: 0;
  }

  .card.ok {
    --tone: 62 207 142;
  }

  .card.bad {
    --tone: 229 72 77;
  }

  @keyframes enter {
    from {
      transform: translateX(40px) scale(0.98);
      opacity: 0;
    }
  }

  .icon {
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    border: 1px solid rgb(var(--tone) / 0.28);
    border-radius: 11px;
    background: linear-gradient(145deg, rgb(var(--tone) / 0.26), rgb(var(--tone) / 0.08));
    color: rgb(var(--tone));
    box-shadow: 0 0 18px -4px rgb(var(--tone) / 0.5);
  }

  .card.calm .icon {
    color: rgb(var(--accent-soft-rgb));
  }

  .body {
    display: grid;
    gap: 2px;
    min-width: 0;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-3);
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.02em;
  }

  .time {
    margin-left: auto;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }

  strong {
    overflow: hidden;
    font-size: 13.5px;
    font-weight: 650;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  p {
    display: -webkit-box;
    margin: 0;
    overflow: hidden;
    color: var(--text-2);
    font-size: 12px;
    line-height: 1.4;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .close {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    margin: -3px -2px 0 0;
    padding: 0;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--text-3);
    cursor: pointer;
    transition:
      background 140ms,
      color 140ms;
  }

  .close:hover {
    background: rgb(255 255 255 / 0.08);
    color: var(--text-1);
  }

  .timer {
    position: absolute;
    right: 0;
    bottom: 0;
    left: 0;
    height: 2px;
    background: linear-gradient(90deg, rgb(var(--tone) / 0.2), rgb(var(--tone) / 0.85));
    transform-origin: left;
  }
</style>
