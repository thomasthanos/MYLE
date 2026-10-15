<script lang="ts">
  import LockKeyhole from "@lucide/svelte/icons/lock-keyhole";
  import MonitorSmartphone from "@lucide/svelte/icons/monitor-smartphone";
  import Fingerprint from "@lucide/svelte/icons/fingerprint";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import Logo from "../lib/components/Logo.svelte";
  import SignIn from "./SignIn.svelte";
</script>

<section class="welcome" aria-labelledby="welcome-title">
  <div class="vault-art" aria-hidden="true">
    <span class="orbit orbit-one"></span><span class="orbit orbit-two"></span>
    <span class="art-key"><KeyRound size={21} /></span>
    <span class="art-fingerprint"><Fingerprint size={24} /></span>
    <div class="vault-mark"><Logo size={98} /></div>
    <span class="art-lock"><LockKeyhole size={17} /></span>
  </div>
  <div class="intro">
    <h1 id="welcome-title">Your passwords.<br />Right here with you.</h1>
    <p>Welcome to MYLE Passwords. Sign in to bring your vault with you, wherever you go.</p>
  </div>
  <div class="connect glass">
    <div class="connect-heading">
      <span class="device-icon"><MonitorSmartphone size={22} /></span>
      <div><h2>One vault. All your devices.</h2><p>Use your MYLE account to get started.</p></div>
    </div>
    <SignIn />
    <p class="account-note">Already use MYLE on your PC? Choose the same account.</p>
  </div>
  <p class="privacy"><LockKeyhole size={15} /><span>Your vault is encrypted before it syncs.<br />Your account never sees your passwords.</span></p>
</section>

<style>
  .welcome { display: flex; flex: none; flex-direction: column; align-items: center; gap: 20px; width: 100%; max-width: 440px; margin: auto; padding: 18px 4px 14px; }
  .vault-art { position: relative; display: grid; place-items: center; flex: none; width: 200px; height: 156px; }
  .orbit { position: absolute; width: 178px; height: 120px; border: 1px solid rgb(var(--accent-rgb) / .18); border-radius: 50%; transform: rotate(-27deg); }
  .orbit-two { width: 146px; height: 146px; border-color: rgb(255 255 255 / .05); transform: rotate(20deg); }
  .vault-mark { display: grid; place-items: center; width: 108px; height: 112px; border: 1px solid rgb(255 255 255 / .12); border-radius: 27px; background: var(--bg-solid-panel); box-shadow: inset 0 1px 0 rgb(255 255 255 / .08), 0 18px 36px -16px #000; transform: rotate(-7deg); }
  .art-key, .art-fingerprint, .art-lock { position: absolute; z-index: 1; display: grid; place-items: center; border: 1px solid rgb(255 255 255 / .14); border-radius: 13px; background: var(--surface-fill); box-shadow: inset 0 1px 0 rgb(255 255 255 / .09), 0 8px 20px -10px #000; color: var(--icon-neutral); }
  .art-key { top: 14px; left: 9px; width: 42px; height: 42px; transform: rotate(-12deg); }
  .art-fingerprint { right: 0; bottom: 20px; width: 48px; height: 48px; transform: rotate(10deg); }
  .art-lock { right: 51px; bottom: 12px; width: 31px; height: 31px; border-radius: 10px; color: rgb(var(--accent-soft-rgb)); }
  .intro { width: 100%; text-align: center; }
  h1 { margin: 0 0 12px; font-family: var(--font-handwritten); font-size: clamp(29px, 8vw, 36px); font-weight: 700; line-height: 1.15; letter-spacing: -.6px; }
  .intro p { max-width: 340px; margin: auto; color: var(--text-2); font-size: 15px; line-height: 1.5; }
  .connect { width: 100%; min-width: 0; padding: 20px 18px 18px; border-radius: 24px; background: linear-gradient(160deg, rgb(255 255 255 / .055), transparent 55%), var(--bg-solid-panel); box-shadow: inset 0 1px 0 rgb(255 255 255 / .07), 0 18px 40px -24px #000; }
  .connect-heading { display: flex; align-items: center; gap: 12px; margin-bottom: 18px; text-align: left; }
  .device-icon { display: grid; place-items: center; flex: none; width: 42px; height: 42px; border: 1px solid rgb(var(--accent-rgb) / .22); border-radius: 13px; background: rgb(var(--accent-rgb) / .1); color: var(--accent); }
  .connect-heading > div { min-width: 0; overflow-wrap: anywhere; }
  h2 { margin: 0 0 4px; font-family: var(--font-sans); font-size: 17px; font-weight: 650; line-height: 1.3; }
  .connect-heading p { color: var(--text-3); font-size: 13px; line-height: 1.4; }
  .account-note { margin-top: 18px; padding-top: 16px; border-top: 1px solid var(--glass-border); color: var(--text-3); font-size: 12.5px; text-align: center; line-height: 1.5; }
  .privacy { display: flex; align-items: flex-start; justify-content: center; gap: 9px; color: var(--text-3); font-size: 12px; line-height: 1.5; text-align: left; }
  .privacy :global(svg) { flex: none; margin-top: 2px; }
  /* Short screens stay compact; all content remains reachable by scrolling. */
  @media (max-height: 800px) { .welcome { gap: 18px; padding-top: 12px; } }
  @media (max-height: 740px) { .welcome { gap: 16px; padding-top: 10px; } .vault-art { height: 120px; transform: scale(.85); margin-block: -6px; } h1 { font-size: 29px; } .connect { padding: 16px; } }
  @media (max-height: 620px) { .welcome { gap: 12px; padding-top: 0; } .vault-art { height: 92px; transform: scale(.66); margin-block: -14px; } h1 { margin-bottom: 6px; font-size: 27px; } .connect { padding: 14px; } .connect-heading { margin-bottom: 10px; } .account-note { margin-top: 10px; padding-top: 10px; } }
  @media (min-width: 650px) { .welcome { padding-top: 40px; gap: 28px; } }
</style>
