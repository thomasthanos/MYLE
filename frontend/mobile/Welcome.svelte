<script lang="ts">
  import LockKeyhole from "@lucide/svelte/icons/lock-keyhole";
  import Logo from "../lib/components/Logo.svelte";
  import SignIn from "./SignIn.svelte";

  let { onlocal }: { onlocal: () => void } = $props();
</script>

<section class="welcome">
  <Logo size={84} />
  <h1>MYLE Passwords</h1>
  <p class="lead">
    Your passwords from MYLE on your PC, on this phone. Sign in with the account you use in MYLE: your vault comes here
    and stays in sync both ways.
  </p>

  <SignIn />

  <button class="link" onclick={onlocal}>No account? Start a new vault on this phone</button>

  <p class="fine">
    <LockKeyhole size={12} />
    Your vault is encrypted on your devices with your master password before it is synced. Nobody else can open it,
    not even your account.
  </p>
</section>

<style>
  .welcome {
    display: grid;
    justify-items: center;
    align-content: center;
    gap: 18px;
    min-height: 100%;
    max-width: 420px;
    margin: 0 auto;
    padding: 24px 4px;
    text-align: center;
  }

  h1 {
    font-size: 26px;
  }

  .lead {
    color: var(--text-2);
    font-size: 15px;
    line-height: 1.5;
  }

  .welcome > :global(.providers),
  .welcome > :global(.waiting) {
    width: 100%;
  }

  .link {
    min-height: 44px;
    color: #b7bef5;
    font-size: 14px;
    text-decoration: underline;
    text-underline-offset: 3px;
  }

  .fine {
    display: flex;
    gap: 8px;
    color: var(--text-3);
    font-size: 12.5px;
    line-height: 1.45;
    text-align: left;
  }

  .fine :global(svg) {
    flex: none;
    margin-top: 2px;
  }
</style>
