<script lang="ts">
  import { onMount, untrack } from "svelte";
  import ClipboardPaste from "@lucide/svelte/icons/clipboard-paste";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import ImageUp from "@lucide/svelte/icons/image-up";
  import Plus from "@lucide/svelte/icons/plus";
  import ScanQrCode from "@lucide/svelte/icons/scan-qr-code";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Star from "@lucide/svelte/icons/star";
  import X from "@lucide/svelte/icons/x";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Popover from "../../../lib/components/Popover.svelte";
  import { confirm } from "../../../lib/confirm.svelte";
  import { MOBILE } from "../../../lib/platform";
  import { scanner } from "../../../mobile/camera.svelte";
  import { passwordsApi as api, type AppLink, type Strength, type TotpInfo } from "./api";
  import Generator from "./Generator.svelte";
  import { iconHost, passwords as p } from "./state.svelte";
  import StrengthMeter from "./StrengthMeter.svelte";

  let { id }: { id: string | null } = $props();

  // Keyed by entry in the parent: a new editor opens for another entry.
  const existing = untrack(() => (id ? p.entry(id) : null));
  /** A new login for the Windows program the hotkey was pressed in. */
  const program = untrack(() => (!id && p.panel.kind === "edit" ? p.panel.program : undefined));
  let title = $state(existing?.title ?? program?.name ?? "");
  let username = $state(existing?.username ?? "");
  /** `null` until typed or loaded: an unchanged password is not sent back. */
  let password = $state<string | null>(null);
  let urls = $state<string[]>(existing?.urls.length ? [...existing.urls] : [""]);
  let apps = $state<AppLink[]>(existing ? existing.apps.map((a) => ({ ...a })) : program ? [{ ...program }] : []);
  let notes = $state(existing?.notes ?? "");
  let folder = $state(existing?.folder ?? "");
  let favorite = $state(existing?.favorite ?? false);
  let show = $state(false);
  let strength = $state<Strength>(existing?.strength ?? "none");
  let generatorOpen = $state(false);
  let newApp = $state("");
  let saving = $state(false);
  let titleInput = $state<HTMLInputElement>();
  /** A new 2FA key or link, typed or scanned; empty keeps the saved one. */
  let totpText = $state("");
  /** The saved key is to go when this is saved. */
  let totpRemoved = $state(false);
  /** Shows the key's field: there is none yet, or the user replaces it. */
  let totpEditing = $state(!existing?.hasTotp);
  let totpInfo = $state<TotpInfo | null>(null);
  let totpError = $state<string | null>(null);
  let scanning = $state(false);
  let checkTimer: ReturnType<typeof setTimeout> | undefined;
  let form = $state<HTMLFormElement>();
  /** The saved password, as loaded: changing it is a change. */
  let loadedPassword = $state<string | null>(null);

  /** What the form says, to tell whether anything was changed. */
  const snapshot = () =>
    JSON.stringify({ title: title.trim(), username, urls: urls.map((u) => u.trim()).filter(Boolean), apps, notes, folder, favorite });
  const initial = untrack(snapshot);
  const dirty = $derived(
    snapshot() !== initial || (password ?? "") !== (loadedPassword ?? "") || !!totpText.trim() || totpRemoved,
  );

  /** Another login with this user name for the same website, if there is one. */
  const duplicate = $derived.by(() => {
    const user = username.trim().toLowerCase();
    const hosts = urls.map(iconHost).filter((h): h is string => !!h);
    if (!user || !hosts.length) return null;
    return (
      p.entries.find(
        (e) => e.id !== id && e.username.trim().toLowerCase() === user && e.urls.some((u) => hosts.includes(iconHost(u) ?? "")),
      ) ?? null
    );
  });

  onMount(async () => {
    // A phone's keyboard would cover the entry being edited.
    if (!MOBILE || !id) titleInput?.focus();
    if (id && existing?.hasPassword) {
      const saved = await api.reveal(id).catch(() => null);
      // Only if nothing was typed meanwhile.
      if (password === null) {
        password = saved;
        loadedPassword = saved;
      }
    }
  });

  /** A new login named after its website, when it has no name yet. */
  function nameFromSite(url: string) {
    if (title.trim()) return;
    const host = iconHost(url.trim());
    if (host) title = host;
  }

  /** Esc leaves (asking first if something changed), Ctrl+S saves. */
  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && !generatorOpen && !e.defaultPrevented) {
      e.preventDefault();
      void cancel();
    } else if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      form?.requestSubmit();
    }
  }

  $effect(() => {
    const value = password ?? "";
    void api.strength(value).then((s) => {
      if (value === (password ?? "")) strength = s;
    });
  });

  function addApp() {
    const raw = newApp.trim().replace(/^"|"$/g, "").replaceAll("/", "\\");
    const exe = raw.split("\\").pop() ?? "";
    if (!exe) return;
    const linked = /^(?:[a-z]:\\|\\\\)/i.test(raw) ? raw : exe;
    const withExe = (name: string) => (name.toLowerCase().endsWith(".exe") ? name : `${name}.exe`);
    const file = withExe(linked);
    // A full path takes the place of the same program added by name only.
    if (linked !== exe) {
      apps = apps.filter((app) => app.exe.toLowerCase() !== withExe(exe).toLowerCase());
    }
    if (!apps.some((a) => a.exe.toLowerCase() === file.toLowerCase())) {
      apps = [...apps, { exe: file, name: exe.replace(/\.exe$/i, "") }];
    }
    newApp = "";
  }

  /** Checks a typed or scanned key with the app (which never sends it back). */
  function checkTotp(text: string, now = false) {
    clearTimeout(checkTimer);
    totpInfo = null;
    totpError = null;
    if (!text.trim()) return;
    checkTimer = setTimeout(
      async () => {
        try {
          const info = await api.totpCheck(text);
          if (text === totpText) totpInfo = info;
        } catch (error) {
          if (text === totpText) totpError = error instanceof Error ? error.message : String(error);
        }
      },
      now ? 0 : 350,
    );
  }

  async function scan(from: "clipboard" | "file" | "camera") {
    scanning = true;
    try {
      const link =
        from === "camera" ? await scanner.readTotp() : from === "clipboard" ? await api.totpScanClipboard() : await api.totpScanFile();
      if (link) {
        totpText = link;
        totpRemoved = false;
        checkTotp(link, true);
      }
    } catch (error) {
      totpInfo = null;
      totpError = error instanceof Error ? error.message : String(error);
    } finally {
      scanning = false;
    }
  }

  const totpBlocks = $derived(!!totpText.trim() && !totpInfo);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!title.trim() || saving || totpBlocks) return;
    saving = true;
    const ok = await p.save({
      id: id ?? undefined,
      title,
      username,
      password: password ?? undefined,
      urls: urls.map((u) => u.trim()).filter(Boolean),
      apps,
      notes,
      folder,
      favorite,
      totp: totpText.trim() ? totpText.trim() : totpRemoved ? "" : undefined,
    });
    saving = false;
    if (ok) password = null;
  }

  async function browseProgram() {
    try {
      const path = await api.pickProgram();
      if (path) {
        newApp = path;
        addApp();
      }
    } catch (error) {
      p.error = error instanceof Error ? error.message : String(error);
    }
  }

  async function cancel() {
    if (
      dirty &&
      !(await confirm({
        title: "Discard your changes?",
        message: id ? "This login stays as it was saved." : "This new login is not added.",
        confirmLabel: "Discard",
        danger: true,
      }))
    ) {
      return;
    }
    p.panel = id ? { kind: "view", id } : { kind: "none" };
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<form class="editor" onsubmit={submit} onkeydown={onKeydown} bind:this={form}>
  <header>
    <h2>{id ? "Edit entry" : "New entry"}</h2>
    <button
      type="button"
      class="icon-btn star"
      class:on={favorite}
      aria-pressed={favorite}
      title="Favorite"
      aria-label="Favorite"
      onclick={() => (favorite = !favorite)}><Star size={16} /></button
    >
  </header>

  <div class="grid">
  <label class="field name">
    <span>Name</span>
    <input class="input" bind:value={title} bind:this={titleInput} placeholder="GitHub, Steam, Bank…" required />
  </label>

  <label class="field user">
    <span>Email or user name</span>
    <input class="input" bind:value={username} autocomplete="off" spellcheck="false" />
    {#if duplicate}
      <small class="duplicate"><TriangleAlert size={12} /> “{duplicate.title}” already has this user name for this website.</small>
    {/if}
  </label>

  <div class="field password">
    <span>Password</span>
    <div class="password-row">
      <input
        class="input mono"
        type={show ? "text" : "password"}
        value={password ?? ""}
        oninput={(e) => (password = e.currentTarget.value)}
        autocomplete="new-password"
        spellcheck="false"
      />
      <button type="button" class="icon-btn" title={show ? "Hide" : "Show"} aria-label={show ? "Hide password" : "Show password"} onclick={() => (show = !show)}>
        {#if show}<EyeOff size={15} />{:else}<Eye size={15} />{/if}
      </button>
      <Popover align="end" bind:open={generatorOpen}>
        {#snippet trigger({ toggle })}
          <button type="button" class="btn small generate" title="Generate a password" onclick={toggle}><Sparkles size={13} /> <span class="label">Generate</span></button>
        {/snippet}
        {#snippet children({ close })}
          <div class="menu">
            <Generator
              onuse={(value) => {
                password = value;
                show = true;
                close();
              }}
            />
          </div>
        {/snippet}
      </Popover>
    </div>
    <StrengthMeter {strength} />
  </div>

  <div class="field totp">
    <span>2FA codes <small>The key the site gives when you turn on an authenticator app</small></span>
    {#if !totpEditing}
      <div class="totp-set">
        {#if totpRemoved}
          <span class="muted">Removed when you save.</span>
          <button type="button" class="link-btn" onclick={() => (totpRemoved = false)}>Undo</button>
        {:else}
          <ShieldCheck size={14} />
          <span>Set up: MYLE shows its codes.</span>
          <button type="button" class="link-btn" onclick={() => (totpEditing = true)}>Replace</button>
          <button type="button" class="link-btn danger" onclick={() => (totpRemoved = true)}>Remove</button>
        {/if}
      </div>
    {:else}
      <div class="row">
        <input
          class="input mono"
          bind:value={totpText}
          oninput={() => checkTotp(totpText)}
          placeholder="Key or otpauth:// link"
          autocomplete="off"
          spellcheck="false"
        />
        {#if MOBILE}
          <button type="button" class="icon-btn" title="Scan the QR code with the camera" aria-label="Scan a QR code" disabled={scanning} onclick={() => scan("camera")}><ScanQrCode size={17} /></button>
        {:else}
          <button type="button" class="icon-btn" title="Read a QR code you snipped or copied (Win+Shift+S)" aria-label="Paste a QR code" disabled={scanning} onclick={() => scan("clipboard")}><ClipboardPaste size={15} /></button>
          <button type="button" class="icon-btn" title="Read a QR code from a picture" aria-label="Open a QR code picture" disabled={scanning} onclick={() => scan("file")}><ImageUp size={15} /></button>
        {/if}
        {#if existing?.hasTotp}
          <button type="button" class="link-btn" onclick={() => ((totpEditing = false), (totpText = ""), checkTotp(""))}>Keep</button>
        {/if}
      </div>
      {#if totpError}
        <p class="totp-note bad">{totpError}</p>
      {:else if totpInfo}
        <p class="totp-note good">
          <ShieldCheck size={12} />
          Codes{totpInfo.issuer || totpInfo.account ? ` for ${[totpInfo.issuer, totpInfo.account].filter(Boolean).join(" · ")}` : ""}:
          {totpInfo.digits} digits every {totpInfo.period} s
        </p>
      {:else if !totpText}
        {#if MOBILE}
          <p class="totp-note">Paste the key, or press <ScanQrCode size={11} /> and point the camera at the site's QR code.</p>
        {:else}
          <p class="totp-note">Paste the key, or snip the QR code (Win+Shift+S) and press <ClipboardPaste size={11} />.</p>
        {/if}
      {/if}
    {/if}
  </div>

  <div class="field sites">
    <span>Websites</span>
    {#each urls as _, i (i)}
      <div class="row">
        <input class="input" bind:value={urls[i]} placeholder="https://example.com" spellcheck="false" onblur={() => i === 0 && nameFromSite(urls[0])} />
        {#if urls.length > 1}
          <button type="button" class="icon-btn" aria-label="Remove website" onclick={() => (urls = urls.filter((_, j) => j !== i))}><X size={14} /></button>
        {/if}
      </div>
    {/each}
    <button type="button" class="add" onclick={() => (urls = [...urls, ""])}><Plus size={13} /> Another website</button>
  </div>

  <!-- Filling Windows programs is MYLE's on the PC; a phone keeps the links as they are. -->
  {#if !MOBILE}
  <div class="field apps-field">
    <span>Windows programs <small>Its full .exe path: then Ctrl+Shift+L in it fills this login</small></span>
    {#if apps.length}
      <div class="apps">
        {#each apps as app (app.exe)}
          <span class="app-chip">
            {app.name} <code>{app.exe}</code>
            <button type="button" aria-label="Unlink {app.name}" onclick={() => (apps = apps.filter((a) => a.exe !== app.exe))}><X size={11} /></button>
          </span>
        {/each}
      </div>
    {/if}
    <div class="row">
      <input
        class="input"
        bind:value={newApp}
        placeholder="C:\Program Files\Riot Games\RiotClientUx.exe"
        spellcheck="false"
        onkeydown={(e) => e.key === "Enter" && (e.preventDefault(), addApp())}
      />
      <button type="button" class="btn small" disabled={!newApp.trim()} onclick={addApp}>Link</button>
      <button type="button" class="btn small" onclick={browseProgram}>Browse…</button>
      {#if p.windowsTarget}
        <button type="button" class="btn small" title={p.windowsTarget.path} onclick={() => ((newApp = p.windowsTarget!.path), addApp())}>Use detected program</button>
      {/if}
    </div>
  </div>
  {/if}

  <label class="field folder">
    <span>Folder</span>
    <input class="input" bind:value={folder} list="password-folders" placeholder="None" />
    <datalist id="password-folders">
      {#each p.folders as name (name)}<option value={name}></option>{/each}
    </datalist>
  </label>

  <label class="field notes-field">
    <span>Notes</span>
    <textarea class="input notes" bind:value={notes} rows="3"></textarea>
  </label>
  </div>

  <footer>
    {#if !MOBILE}<span class="keys">Ctrl+S saves · Esc cancels</span>{/if}
    <button type="button" class="btn ghost" onclick={cancel}>Cancel</button>
    <button type="submit" class="btn primary" disabled={!title.trim() || saving || totpBlocks}>{id ? "Save" : "Add to vault"}</button>
  </footer>
</form>

<style>
  /* The same readable width as the entry it edits; two columns when the
     panel is wide enough, so the whole form shows at 1080p. */
  .editor {
    display: grid;
    gap: 14px;
    width: min(100%, 760px);
    margin: 0 auto;
    container: editor / inline-size;
  }

  .grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 14px;
    align-items: start;
  }

  @container editor (min-width: 600px) {
    .grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
      grid-template-areas:
        "name folder"
        "user password"
        "sites apps"
        "totp notes";
      column-gap: 16px;
    }

    .name { grid-area: name; }
    .folder { grid-area: folder; }
    .user { grid-area: user; }
    .password { grid-area: password; }
    .sites { grid-area: sites; }
    .apps-field { grid-area: apps; }
    .notes-field { grid-area: notes; }
    .totp { grid-area: totp; }
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  h2 {
    font-size: 17px;
  }

  .star.on {
    color: #ffd166;
  }

  .field {
    display: grid;
    gap: 6px;
  }

  .field > span {
    color: var(--text-2);
    font-size: 12px;
    font-weight: 600;
  }

  .field small {
    margin-left: 6px;
    color: var(--text-3);
    font-size: 10.5px;
    font-weight: 400;
  }

  .password-row,
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .password-row .input,
  .row .input {
    flex: 1;
    min-width: 0;
  }

  .mono {
    font-family: var(--font-mono);
  }

  .field .duplicate {
    display: flex;
    align-items: center;
    gap: 5px;
    margin: 0;
    color: #ffd18f;
    font-size: 11.5px;
  }

  footer .keys {
    margin-right: auto;
    align-self: center;
    color: var(--text-3);
    font-size: 11px;
  }

  /* A phone's narrow row: the generator by its icon only. */
  :global(html.mobile) .generate {
    width: 40px;
    height: 40px;
    padding: 0;
    justify-content: center;
  }

  :global(html.mobile) .generate .label {
    display: none;
  }

  .menu {
    padding: 0;
  }

  .add {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    justify-self: start;
    color: var(--text-3);
    font-size: 12px;
  }

  .add:hover {
    color: var(--text-1);
  }

  .apps {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .app-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 100%;
    min-width: 0;
    padding: 3px 5px 3px 10px;
    border: 1px solid rgb(var(--accent-rgb) / 0.25);
    border-radius: 999px;
    background: rgb(var(--accent-rgb) / 0.1);
    font-size: 12px;
  }

  .app-chip code {
    min-width: 0;
    overflow: hidden;
    color: var(--text-3);
    font-size: 10.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .app-chip button {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    flex: none;
    border-radius: 50%;
    color: var(--text-3);
  }

  .app-chip button:hover {
    background: var(--hover);
    color: var(--text-1);
  }

  .totp-set {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 34px;
    padding: 0 12px;
    border: 1px solid rgb(74 222 128 / 0.22);
    border-radius: 10px;
    background: rgb(74 222 128 / 0.06);
    color: #86efac;
    font-size: 12.5px;
  }

  .totp-set span {
    flex: 1;
    color: var(--text-2);
  }

  .totp-set .muted {
    color: var(--text-3);
  }

  .link-btn {
    color: #b9c2ff;
    font-size: 12px;
  }

  .link-btn:hover {
    text-decoration: underline;
  }

  .link-btn.danger {
    color: #ff9d9d;
  }

  .totp-note {
    display: flex;
    align-items: center;
    gap: 5px;
    margin: 0;
    color: var(--text-3);
    font-size: 11.5px;
    line-height: 1.35;
  }

  .totp-note.good {
    color: #86efac;
  }

  .totp-note.bad {
    color: #ffb6a8;
  }

  .notes {
    height: auto;
    padding: 9px 12px;
    resize: vertical;
    line-height: 1.45;
  }

  /* Save stays in sight while the form scrolls. */
  footer {
    position: sticky;
    bottom: 0;
    z-index: 2;
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin: 0 -10px -6px;
    padding: 10px 10px 6px;
    border-top: 1px solid rgb(255 255 255 / 0.06);
    background: var(--solid-fill-bottom);
    border-radius: 0 0 12px 12px;
  }

  @media (max-height: 1000px) {
    .editor,
    .grid {
      gap: 10px;
    }

    .field {
      gap: 4px;
    }

    .notes {
      min-height: 58px;
      height: 58px;
    }
  }
</style>
