export function renderRequestLogsView(config) {
  const {
    dom,
    state,
    windowState,
    ensureBindings,
    ensureAccountLabelMap,
    collectFilteredRequestLogs,
    isAppendOnlyResult,
    isNearBottom,
    appendAtLeastOneBatch,
    renderEmptyRequestLogs,
    createTopSpacerRow,
    columnCount,
    scrollBuffer,
    nearBottomMaxBatches,
  } = config;

  if (!dom.requestLogRows) {
    return;
  }

  ensureBindings();
  ensureAccountLabelMap(state.accountList, windowState);
  const filter = state.requestLogStatusFilter || "all";
  const { filtered, filteredKeys } = collectFilteredRequestLogs(
    state.requestLogList,
    filter,
  );
  const sameFilter = filter === windowState.filter;
  const appendOnly = sameFilter && isAppendOnlyResult(
    windowState.filteredKeys,
    filteredKeys,
  );
  const unchanged = appendOnly && filteredKeys.length === windowState.filteredKeys.length;
  const canReuseRenderedDom = filtered.length > 0
    ? Boolean(windowState.topSpacerRow && dom.requestLogRows.contains(windowState.topSpacerRow))
    : dom.requestLogRows.children.length > 0;

  if (windowState.hasRendered && canReuseRenderedDom && unchanged) {
    windowState.filtered = filtered;
    windowState.filteredKeys = filteredKeys;
    return;
  }

  if (
    windowState.hasRendered
    && appendOnly
    && windowState.topSpacerRow
    && dom.requestLogRows.contains(windowState.topSpacerRow)
  ) {
    const previousLength = windowState.filtered.length;
    windowState.filtered = filtered;
    windowState.filteredKeys = filteredKeys;
    windowState.filter = filter;
    if (
      windowState.nextIndex >= previousLength
      || isNearBottom(windowState.boundScrollerEl, scrollBuffer)
    ) {
      appendAtLeastOneBatch({
        scroller: windowState.boundScrollerEl,
        scrollBuffer,
        nearBottomMaxBatches,
      });
    }
    return;
  }

  dom.requestLogRows.innerHTML = "";
  windowState.filtered = filtered;
  windowState.filteredKeys = filteredKeys;
  windowState.filter = filter;
  windowState.nextIndex = 0;
  windowState.topSpacerHeight = 0;
  windowState.recycledRowHeight = 54;
  windowState.topSpacerRow = null;
  windowState.topSpacerCell = null;
  windowState.hasRendered = true;

  if (!filtered.length) {
    renderEmptyRequestLogs(dom.requestLogRows, columnCount);
    return;
  }

  dom.requestLogRows.appendChild(
    createTopSpacerRow({
      columnCount,
      windowState,
    }),
  );
  appendAtLeastOneBatch({
    scroller: windowState.boundScrollerEl,
    extraMaxBatches: 1,
    scrollBuffer,
    nearBottomMaxBatches,
  });
}
