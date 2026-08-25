(function exposeEngineSelection(root, factory) {
  const api = factory();
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.FileFlowEngineSelection = api;
})(globalThis, () => {
  function summarize(engines, selectedValues) {
    const selected = new Set(selectedValues);
    const actionable = engines.filter((engine) => selected.has(engine.id));
    return {
      selected: actionable.length,
      total: engines.length,
      missing: actionable.filter((engine) => !engine.installed).length,
      allSelected: engines.length > 0 && actionable.length === engines.length,
    };
  }

  function toggleAll(engines, selectedValues) {
    const current = new Set(selectedValues);
    const status = summarize(engines, current);
    return status.allSelected ? [] : engines.map((engine) => engine.id);
  }

  function selectMissingByDefault(engines, selectedValues) {
    const current = [...selectedValues];
    if (current.length > 0) return current;
    const missing = engines.filter((engine) => !engine.installed).map((engine) => engine.id);
    return missing.length > 0 ? missing : engines.map((engine) => engine.id);
  }

  return { summarize, toggleAll, selectMissingByDefault };
});
