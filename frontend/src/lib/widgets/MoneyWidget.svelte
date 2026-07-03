<script lang="ts">
  let {
    value,
    onchange,
  }: {
    value: { amount: number; currency: string } | null;
    onchange: (v: { amount: number; currency: string } | null) => void;
  } = $props();

  const cur = $derived(value?.currency ?? "KRW");
  const amt = $derived(value?.amount ?? null);

  function setAmount(s: string) {
    if (s === "") {
      onchange(null);
      return;
    }
    onchange({ amount: Number(s), currency: cur });
  }

  function setCurrency(c: string) {
    onchange({ amount: amt ?? 0, currency: c });
  }
</script>

<span class="money">
  <input
    type="number"
    placeholder="금액"
    value={amt ?? ""}
    oninput={(e) => setAmount((e.currentTarget as HTMLInputElement).value)}
  />
  <input
    placeholder="통화"
    value={cur}
    oninput={(e) => setCurrency((e.currentTarget as HTMLInputElement).value)}
  />
</span>
