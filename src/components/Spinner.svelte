<script lang="ts">
  let { size = 12 }: { size?: number } = $props();
</script>

<span class="spin" style:width="{size}px" style:height="{size}px" aria-hidden="true"></span>

<style>
  .spin {
    display: inline-block;
    flex-shrink: 0;
    border-radius: 50%;
    border: 1.5px solid var(--line2);
    border-top-color: var(--faint);
    animation: rot 0.8s linear infinite;
  }
  @keyframes rot {
    to {
      transform: rotate(360deg);
    }
  }
</style>
