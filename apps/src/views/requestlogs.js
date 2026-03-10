import { dom } from "../ui/dom.js";
import { state } from "../state.js";
import { copyText } from "../utils/clipboard.js";
import {
  createRequestLogRow,
  createTopSpacerRow,
  renderEmptyRequestLogs,
} from "./requestlogs/row-render.js";
import {
  buildRequestRouteMeta,
  collectFilteredRequestLogs,
  ensureAccountLabelMap,
  fallbackAccountDisplayFromKey,
  isAppendOnlyResult,
  resolveAccountDisplayName,
  resolveDisplayRequestPath,
} from "./requestlogs/selectors.js";
import {
  appendAtLeastOneBatch,
  appendNearBottomBatches,
  appendRequestLogBatch,
  isNearBottom,
} from "./requestlogs/virtual-list.js";
import { createRequestLogBindings } from "./requestlogs/events.js";
import { renderRequestLogsView } from "./requestlogs/render-requestlogs.js";

const REQUEST_LOG_BATCH_SIZE = 80;
const REQUEST_LOG_DOM_LIMIT = 240;
const REQUEST_LOG_DOM_RECYCLE_TO = 180;
const REQUEST_LOG_SCROLL_BUFFER = 180;
const REQUEST_LOG_FALLBACK_ROW_HEIGHT = 54;
const REQUEST_LOG_COLUMN_COUNT = 9;
const REQUEST_LOG_NEAR_BOTTOM_MAX_BATCHES = 1;

const requestLogWindowState = {
  filter: "all",
  filtered: [],
  filteredKeys: [],
  nextIndex: 0,
  topSpacerHeight: 0,
  recycledRowHeight: REQUEST_LOG_FALLBACK_ROW_HEIGHT,
  accountListRef: null,
  accountLabelById: new Map(),
  topSpacerRow: null,
  topSpacerCell: null,
  boundRowsEl: null,
  boundScrollerEl: null,
  scrollTickHandle: null,
  scrollTickMode: "",
  hasRendered: false,
};

function createRowRenderer() {
  const accountLabelById = requestLogWindowState.accountLabelById;
  const rowRenderHelpers = {
    resolveAccountDisplayName: (item) =>
      resolveAccountDisplayName(item, accountLabelById),
    fallbackAccountDisplayFromKey,
    resolveDisplayRequestPath,
    buildRequestRouteMeta,
  };
  return (item, index) => createRequestLogRow(item, index, rowRenderHelpers);
}

function appendRequestLogBatchLocal() {
  return appendRequestLogBatch({
    rowsEl: dom.requestLogRows,
    windowState: requestLogWindowState,
    batchSize: REQUEST_LOG_BATCH_SIZE,
    createRow: createRowRenderer(),
    domLimit: REQUEST_LOG_DOM_LIMIT,
    domRecycleTo: REQUEST_LOG_DOM_RECYCLE_TO,
    fallbackRowHeight: REQUEST_LOG_FALLBACK_ROW_HEIGHT,
  });
}

const requestLogBindings = createRequestLogBindings({
  dom,
  windowState: requestLogWindowState,
  copyText,
  resolveDisplayRequestPath,
  isNearBottom,
  appendNearBottomBatches,
  scrollBuffer: REQUEST_LOG_SCROLL_BUFFER,
  nearBottomMaxBatches: REQUEST_LOG_NEAR_BOTTOM_MAX_BATCHES,
  appendRequestLogBatch: appendRequestLogBatchLocal,
});

export function renderRequestLogs() {
  renderRequestLogsView({
    dom,
    state,
    windowState: requestLogWindowState,
    ensureBindings: requestLogBindings.ensureRequestLogBindings,
    ensureAccountLabelMap,
    collectFilteredRequestLogs,
    isAppendOnlyResult,
    isNearBottom,
    appendAtLeastOneBatch: ({ scroller, extraMaxBatches, scrollBuffer, nearBottomMaxBatches }) =>
      appendAtLeastOneBatch({
        scroller,
        extraMaxBatches,
        scrollBuffer,
        nearBottomMaxBatches,
        appendRequestLogBatch: appendRequestLogBatchLocal,
      }),
    renderEmptyRequestLogs,
    createTopSpacerRow,
    columnCount: REQUEST_LOG_COLUMN_COUNT,
    scrollBuffer: REQUEST_LOG_SCROLL_BUFFER,
    nearBottomMaxBatches: REQUEST_LOG_NEAR_BOTTOM_MAX_BATCHES,
  });
}
