<script lang="ts">
  // Диалог из askDialog()/confirmDialog() (lib/ui.svelte.ts).
  import Button from "./Button.svelte";
  import Modal from "./Modal.svelte";
  import { closeConfirm, ui } from "../lib/ui.svelte";
</script>

{#if ui.confirm}
  {@const c = ui.confirm}
  <Modal title={c.title} onclose={() => closeConfirm("cancel")}>
    <p>{c.text}</p>
    {#snippet buttons()}
      <Button onclick={() => closeConfirm("cancel")}>{c.cancel ?? "Отмена"}</Button>
      {#if c.alt}<Button onclick={() => closeConfirm("alt")}>{c.alt}</Button>{/if}
      <Button variant={c.danger ? "danger" : "primary"} onclick={() => closeConfirm("ok")}>{c.ok}</Button>
    {/snippet}
  </Modal>
{/if}

<style>
  p {
    margin: 0;
    white-space: pre-line;
  }
</style>
