let context: AudioContext | undefined;
let clip: AudioBuffer | undefined;
let loading = false;
let activeGain: GainNode | undefined;

// An accent from the original cleanup track, with room for its decay.
const cueOffset = 4.44;
const cueLength = 1.03;

export function prepareCleanAudio(): void {
  try {
    const audio = context ??= new AudioContext();
    // Resume from the confirmation click so the cue can play after the async cleanup.
    if (audio.state === "suspended") void audio.resume().catch(() => {});
    if (loading || clip) return;

    loading = true;
    void fetch("/sounds/clean-complete.mp3")
      .then((response) => {
        if (!response.ok) throw new Error("Cleanup audio unavailable");
        return response.arrayBuffer();
      })
      .then((data) => audio.decodeAudioData(data))
      .then((decoded) => { clip = decoded; })
      .catch(() => {})
      .finally(() => { loading = false; });
  } catch {
    // Cleanup can run without audio.
  }
}

export function playCleanComplete(): void {
  const audio = context;
  if (!audio || audio.state !== "running") return;

  stopCleanAudio();
  const gain = audio.createGain();
  activeGain = gain;
  gain.connect(audio.destination);

  const now = audio.currentTime;
  const release = () => {
    if (activeGain === gain) stopCleanAudio();
  };
  if (clip && clip.duration >= cueOffset + cueLength) {
    const level = 0.14;
    const source = audio.createBufferSource();
    source.buffer = clip;
    source.connect(gain);
    source.onended = release;

    gain.gain.setValueAtTime(0, now);
    gain.gain.linearRampToValueAtTime(level, now + 0.065);
    gain.gain.setValueAtTime(level, now + cueLength - 0.28);
    gain.gain.linearRampToValueAtTime(0, now + cueLength);
    source.start(now, cueOffset, cueLength);
    return;
  }

  // A short fallback keeps the cue in sync when the asset is still loading.
  gain.gain.setValueAtTime(0.012, now);
  for (const [index, frequency] of [659.25, 880, 1046.5].entries()) {
    const start = now + index * 0.07;
    const end = start + 0.32;
    const note = audio.createOscillator();
    const envelope = audio.createGain();
    note.frequency.value = frequency;
    envelope.gain.setValueAtTime(0.001, start);
    envelope.gain.exponentialRampToValueAtTime(0.9, start + 0.025);
    envelope.gain.exponentialRampToValueAtTime(0.001, end);
    note.connect(envelope);
    envelope.connect(gain);
    if (index === 2) note.onended = release;
    note.start(start);
    note.stop(end);
  }
}

export function stopCleanAudio(): void {
  activeGain?.disconnect();
  activeGain = undefined;
}

