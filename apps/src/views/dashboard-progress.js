import { formatResetLabel, remainingPercent } from "../utils/format";

export function buildProgressLine(label, usedPercent, resetsAt, secondary) {
  const remain = remainingPercent(usedPercent);
  const line = document.createElement("div");
  line.className = "progress-line";
  if (secondary) line.classList.add("secondary");
  const lineLabel = document.createElement("span");
  lineLabel.textContent = `${label} ${remain == null ? "--" : `${remain}%`}`;
  const track = document.createElement("div");
  track.className = "track";
  const fill = document.createElement("div");
  fill.className = "fill";
  fill.style.width = remain == null ? "0%" : `${remain}%`;
  track.appendChild(fill);
  line.appendChild(lineLabel);
  line.appendChild(track);

  const wrap = document.createElement("div");
  wrap.appendChild(line);
  if (resetsAt) {
    const reset = document.createElement("div");
    reset.className = "hint";
    reset.textContent = formatResetLabel(resetsAt);
    wrap.appendChild(reset);
  }
  return wrap;
}
