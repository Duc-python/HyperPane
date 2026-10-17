import { useEffect, useRef, useState } from "react";
import type { LayoutNode, SplitDirection } from "../lib/ipc";
import { TerminalPane } from "./TerminalPane";

interface LayoutViewProps {
  node: LayoutNode;
  /** Steps from the root split to this node; true descends into `first`. */
  path: boolean[];
  focused: string | null;
  canClose: boolean;
  onFocus: (id: string) => void;
  onClose: (id: string) => void;
  onRatioCommit: (path: boolean[], ratio: number) => void;
}

export function LayoutView({
  node,
  path,
  focused,
  canClose,
  onFocus,
  onClose,
  onRatioCommit,
}: LayoutViewProps) {
  if (node.type === "leaf") {
    return (
      <TerminalPane
        id={node.pane}
        focused={focused === node.pane}
        canClose={canClose}
        onFocus={onFocus}
        onClose={onClose}
      />
    );
  }

  return (
    <SplitView
      node={node}
      path={path}
      focused={focused}
      canClose={canClose}
      onFocus={onFocus}
      onClose={onClose}
      onRatioCommit={onRatioCommit}
    />
  );
}

type SplitViewProps = Omit<LayoutViewProps, "node"> & {
  node: Extract<LayoutNode, { type: "split" }>;
};

/**
 * One split: two children plus the divider between them.
 *
 * The ratio is held locally while a divider is dragged and committed to the
 * backend once on release, so dragging never queues an IPC round trip per
 * pointer move.
 */
function SplitView({
  node,
  path,
  focused,
  canClose,
  onFocus,
  onClose,
  onRatioCommit,
}: SplitViewProps) {
  const [ratio, setRatio] = useState(node.ratio);

  useEffect(() => {
    setRatio(node.ratio);
  }, [node.ratio]);

  const isRow = node.direction === "row";

  return (
    <div
      className={`flex h-full w-full ${isRow ? "flex-row" : "flex-col"}`}
    >
      <div className="min-h-0 min-w-0" style={{ flexGrow: ratio, flexBasis: 0 }}>
        <LayoutView
          node={node.first}
          path={[...path, true]}
          focused={focused}
          canClose={canClose}
          onFocus={onFocus}
          onClose={onClose}
          onRatioCommit={onRatioCommit}
        />
      </div>

      <Divider
        direction={node.direction}
        ratio={ratio}
        onChange={setRatio}
        onCommit={(value) => onRatioCommit(path, value)}
      />

      <div
        className="min-h-0 min-w-0"
        style={{ flexGrow: 1 - ratio, flexBasis: 0 }}
      >
        <LayoutView
          node={node.second}
          path={[...path, false]}
          focused={focused}
          canClose={canClose}
          onFocus={onFocus}
          onClose={onClose}
          onRatioCommit={onRatioCommit}
        />
      </div>
    </div>
  );
}

interface DividerProps {
  direction: SplitDirection;
  ratio: number;
  onChange: (ratio: number) => void;
  onCommit: (ratio: number) => void;
}

/** Ratio bounds; the backend clamps to the same range. */
const MIN_RATIO = 0.1;
const MAX_RATIO = 0.9;
const KEY_STEP = 0.02;

function Divider({ direction, ratio, onChange, onCommit }: DividerProps) {
  const ref = useRef<HTMLDivElement | null>(null);
  const [dragging, setDragging] = useState(false);
  const isRow = direction === "row";

  const clamp = (value: number) =>
    Math.min(MAX_RATIO, Math.max(MIN_RATIO, value));

  const ratioAt = (clientX: number, clientY: number): number | null => {
    const parent = ref.current?.parentElement;
    if (!parent) return null;
    const rect = parent.getBoundingClientRect();
    const span = isRow ? rect.width : rect.height;
    if (span <= 0) return null;
    const offset = isRow ? clientX - rect.left : clientY - rect.top;
    return clamp(offset / span);
  };

  return (
    <div
      ref={ref}
      role="separator"
      tabIndex={0}
      aria-orientation={isRow ? "vertical" : "horizontal"}
      aria-label="Resize panes"
      aria-valuenow={Math.round(ratio * 100)}
      aria-valuemin={Math.round(MIN_RATIO * 100)}
      aria-valuemax={Math.round(MAX_RATIO * 100)}
      className={`group/divider relative shrink-0 ${
        isRow ? "w-1.5 cursor-col-resize" : "h-1.5 cursor-row-resize"
      }`}
      onPointerDown={(event) => {
        event.currentTarget.setPointerCapture(event.pointerId);
        setDragging(true);
      }}
      onPointerMove={(event) => {
        if (!dragging) return;
        const next = ratioAt(event.clientX, event.clientY);
        if (next !== null) onChange(next);
      }}
      onPointerUp={(event) => {
        event.currentTarget.releasePointerCapture(event.pointerId);
        setDragging(false);
        onCommit(ratio);
      }}
      onKeyDown={(event) => {
        const back = isRow ? "ArrowLeft" : "ArrowUp";
        const forward = isRow ? "ArrowRight" : "ArrowDown";
        if (event.key !== back && event.key !== forward) return;
        event.preventDefault();
        const next = clamp(ratio + (event.key === forward ? KEY_STEP : -KEY_STEP));
        onChange(next);
        onCommit(next);
      }}
    >
      <span
        aria-hidden
        className={`absolute rounded-full bg-line transition-colors group-hover/divider:bg-accent-strong ${
          isRow
            ? "top-0 bottom-0 left-1/2 w-px -translate-x-1/2"
            : "left-0 right-0 top-1/2 h-px -translate-y-1/2"
        }`}
      />
    </div>
  );
}
