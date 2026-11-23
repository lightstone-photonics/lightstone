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
  <a class="gh" href="https://github.com/lightstone-photonics/lightstone" target="_blank" rel="noopener noreferrer"
     title="Source code, documentation and validation on GitHub">
    <!-- GitHub mark (octicon mark-github, MIT) -->
    <svg viewBox="0 0 16 16" width="22" height="22" aria-hidden="true" focusable="false">
      <path fill="currentColor" d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/>
    </svg>
    <span>GitHub</span>
  </a>
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
  .top { margin-bottom: 0.6rem; display: flex; align-items: center; justify-content: space-between; gap: 1rem; }
  .gh { display: inline-flex; align-items: center; gap: 0.4rem; color: #c9d1e0; text-decoration: none; font-size: 0.9rem;
        padding: 0.3rem 0.6rem; border: 1px solid #39425a; border-radius: 6px; }
  .gh:hover { color: #fff; border-color: #6c7896; background: #1d2333; }
  .gh:focus-visible { outline: 2px solid #f6a03a; outline-offset: 2px; }
  .loading { color: #8a93a8; }
  footer { font-size: 0.8rem; color: #8a93a8; margin-top: 1rem; }
  .skip { position: absolute; left: -9999px; }
  .skip:focus { left: 1rem; top: 1rem; background: #f6a03a; color: #000; padding: 0.3rem 0.6rem; }
  :global(.sr-only) { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
</style>
