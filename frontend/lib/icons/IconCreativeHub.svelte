<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, strokeWidth = 1.75, class: cls = "" }: {
    size?: number;
    strokeWidth?: number;
    class?: string;
  } = $props();
</script>

<IconCanvas {size} {strokeWidth} class={cls}>
  <!-- Smooth porcelain Anonymous mask face with translucent glass fill -->
  <path d="M4 5.2C4 3.2 20 3.2 20 5.2C20 9.5 20.8 12.8 18 16.8L12 22L6 16.8C3.2 12.8 4 9.5 4 5.2Z" />
  <!-- Prominently arched, expressive eyebrows -->
  <path d="M5.5 7.8C7.2 5.5 9.8 5.8 11 7.5M18.5 7.8C16.8 5.5 14.2 5.8 13 7.5" stroke-width="2" />
  <!-- Sleek eye slits -->
  <path d="M6.5 10h3.5M14 10h3.5" stroke-width="1.8" />
  <!-- Iconic Guy Fawkes handlebar mustache with smooth upward cheek sweeps -->
  <path d="M4 12.8C5.5 15.5 8.5 15.8 12 14.2C15.5 15.8 18.5 15.5 20 12.8" stroke-width="2.2" />
  <!-- Subtle wry smirk -->
  <path d="M9.8 16.5C10.8 17.5 13.2 17.5 14.2 16.5" stroke-width="1.5" />
  <!-- Rounded vertical goatee drop -->
  <path d="M12 18v2.8" stroke-width="2.2" />
</IconCanvas>
