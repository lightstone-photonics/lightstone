<script>
  import { onMount } from 'svelte';
  import { createEngine } from './lib/engine.js';
  import { docs } from './lib/docs.js';
  import GraphEditor from './lib/graph/GraphEditor.svelte';

  let engine = $state.raw(null);
  let init = $state(null); // { kinds, reference, engineVersion } from the worker
  let graph = $state(null); // the editor's graph document
  let study = $state(null); // recorded runs

  onMount(async () => {
    engine = createEngine();
    const ready = await engine.ready;
    init = { kinds: ready.kinds, reference: ready.referenceGraph, engineVersion: ready.engineVersion, fiberPresets: ready.fiberPresets };
  });
</script>

<a class="skip" href="#editor">Skip to the editor</a>
<header class="top">
  <h1><img class="logo" src="./logo.svg" alt="" width="40" height="40" />Lightstone <span>Optical Link Simulator</span></h1>
</header>

<main id="editor">
  {#if init}
    <GraphEditor {engine} kinds={init.kinds} reference={init.reference} engineVersion={init.engineVersion} fiberPresets={init.fiberPresets} {docs} bind:graph bind:study />
  {:else}
    <p class="loading" role="status">Loading WebAssembly…</p>
  {/if}
</main>

<footer>
  <p>
    Model limits apply; see each component's "Assumptions & limits". Current domain: 850 nm VCSEL, OM4 multimode fiber,
    PIN/TIA, PAM-4 diagnostics (docs/architecture.md). TDECQ follows the IEEE method with MMSE taps (validation V19).
  </p>
</footer>

<style>
  .top h1 { margin: 0.2rem 0; font-size: 1.5rem; display: flex; align-items: center; gap: 0.55rem; }
  .top h1 .logo { width: 40px; height: 40px; flex: none; }
  .top h1 span { font-weight: 400; color: #aab3c5; font-size: 1rem; margin-left: 0.1rem; }
  .top { margin-bottom: 0.6rem; }
  .loading { color: #8a93a8; }
  footer { font-size: 0.8rem; color: #8a93a8; margin-top: 1rem; }
  .skip { position: absolute; left: -9999px; }
  .skip:focus { left: 1rem; top: 1rem; background: #f6a03a; color: #000; padding: 0.3rem 0.6rem; }
  :global(.sr-only) { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
</style>
