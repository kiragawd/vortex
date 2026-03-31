import { useState, useMemo, useCallback } from 'react';
import type { DagTask, TaskInstance } from '../api/client';

const NODE_W = 148;
const NODE_H = 44;
const LAYER_GAP_X = 180;
const NODE_GAP_Y = 60;
const PADDING = 24;
const MAX_VISIBLE_NODES = 25;

const STATE_COLORS: Record<string, string> = {
  success: '#10b981',
  running: '#3b82f6',
  failed: '#ef4444',
  pending: '#f59e0b',
  queued: '#6b7280',
  skipped: '#9ca3af',
};

const TYPE_COLORS: Record<string, string> = {
  bash: '#6366f1',
  python: '#8b5cf6',
  sql: '#0ea5e9',
  http: '#10b981',
  email: '#f59e0b',
  sensor: '#ec4899',
  default: '#6366f1',
};

interface DagGraphProps {
  tasks: DagTask[];
  /** Backend serialises Vec<(up, down)> as [[up, down], ...] */
  dependencies: [string, string][];
  /** task_name → state for per-run highlighting */
  instancesByTask?: Record<string, string>;
  onTaskClick?: (taskName: string) => void;
}

function assignLayers(taskNames: string[], deps: [string, string][]): Record<string, number> {
  const upstreams: Record<string, string[]> = {};
  taskNames.forEach((n) => { upstreams[n] = []; });
  deps.forEach(([up, down]) => {
    if (!upstreams[down]) upstreams[down] = [];
    upstreams[down].push(up);
  });

  const layer: Record<string, number> = {};
  const visiting = new Set<string>();

  function getLayer(name: string): number {
    if (name in layer) return layer[name];
    if (visiting.has(name)) return 0; // cycle guard
    visiting.add(name);
    const ups = upstreams[name] ?? [];
    layer[name] = ups.length === 0 ? 0 : Math.max(...ups.map(getLayer)) + 1;
    visiting.delete(name);
    return layer[name];
  }

  taskNames.forEach(getLayer);
  return layer;
}

/**
 * When the backend returns empty dependencies (e.g. DAGs loaded from DB without
 * persisted intra-task edges), infer a simple sequential chain from task order.
 * This gives a reasonable visual layout instead of all tasks stacked in one column.
 */
function inferSequentialDeps(tasks: DagTask[]): [string, string][] {
  if (tasks.length <= 1) return [];
  const deps: [string, string][] = [];
  for (let i = 0; i < tasks.length - 1; i++) {
    deps.push([tasks[i].name, tasks[i + 1].name]);
  }
  return deps;
}

/** Collect all downstream node names reachable from `startNode` */
function getDownstream(startNode: string, deps: [string, string][]): Set<string> {
  const children: Record<string, string[]> = {};
  deps.forEach(([up, down]) => {
    if (!children[up]) children[up] = [];
    children[up].push(down);
  });
  const result = new Set<string>();
  const queue = [startNode];
  while (queue.length > 0) {
    const node = queue.pop()!;
    for (const child of (children[node] ?? [])) {
      if (!result.has(child)) {
        result.add(child);
        queue.push(child);
      }
    }
  }
  return result;
}

/** BFS from root nodes to select the first `limit` nodes as a connected subgraph */
function selectRootBFS(tasks: DagTask[], deps: [string, string][], limit: number): DagTask[] {
  const taskNames = new Set(tasks.map((t) => t.name));
  const hasUpstream = new Set<string>();
  const children: Record<string, string[]> = {};
  deps.forEach(([up, down]) => {
    if (taskNames.has(down)) hasUpstream.add(down);
    if (!children[up]) children[up] = [];
    children[up].push(down);
  });
  // Find root nodes (no upstream edges)
  const roots = tasks.filter((t) => !hasUpstream.has(t.name));
  // BFS from roots
  const visited = new Set<string>();
  const result: DagTask[] = [];
  const queue = roots.map((t) => t.name);
  // If no roots found (cycle), fall back to first tasks
  if (queue.length === 0) {
    return tasks.slice(0, limit);
  }
  while (queue.length > 0 && result.length < limit) {
    const name = queue.shift()!;
    if (visited.has(name)) continue;
    visited.add(name);
    const task = tasks.find((t) => t.name === name);
    if (task) result.push(task);
    for (const child of (children[name] ?? [])) {
      if (!visited.has(child)) queue.push(child);
    }
  }
  return result;
}

export function DagGraph({ tasks, dependencies, instancesByTask = {}, onTaskClick }: DagGraphProps) {
  const [expanded, setExpanded] = useState(false);
  // When a node is clicked in the collapsed view, show it + its downstream nodes
  const [focusedNode, setFocusedNode] = useState<string | null>(null);

  // Build id→name map so dependency IDs (from backend) resolve to task names.
  // For Python DAGs id==name so this is a no-op; for YAML/config DAGs they differ.
  const idToName = useMemo(() => {
    const map: Record<string, string> = {};
    tasks.forEach((t) => { map[t.id] = t.name; });
    return map;
  }, [tasks]);

  // Map backend deps (which reference task IDs) to task names, or infer sequential deps
  const effectiveDeps = useMemo(() => {
    if (dependencies.length > 0) {
      return dependencies.map(([up, down]) => [
        idToName[up] ?? up,
        idToName[down] ?? down,
      ] as [string, string]);
    }
    return inferSequentialDeps(tasks);
  }, [tasks, dependencies, idToName]);

  const totalNodes = tasks.length;
  const isLargeGraph = totalNodes > MAX_VISIBLE_NODES;

  // Build downstream map for the focused node
  const downstreamOfFocus = useMemo(() => {
    if (!focusedNode) return new Set<string>();
    return getDownstream(focusedNode, effectiveDeps);
  }, [focusedNode, effectiveDeps]);

  // Determine which tasks to display
  const visibleTasks = useMemo(() => {
    if (!isLargeGraph || expanded) return tasks;
    if (focusedNode) {
      // Show the focused node + all its downstream
      const names = new Set<string>([focusedNode, ...downstreamOfFocus]);
      return tasks.filter((t) => names.has(t.name));
    }
    // Default collapsed view: BFS from root nodes so roots are always visible
    return selectRootBFS(tasks, effectiveDeps, MAX_VISIBLE_NODES);
  }, [tasks, isLargeGraph, expanded, focusedNode, downstreamOfFocus, effectiveDeps]);

  const visibleNames = useMemo(() => new Set(visibleTasks.map((t) => t.name)), [visibleTasks]);

  // Only show edges between visible nodes
  const visibleDeps = useMemo(
    () => effectiveDeps.filter(([up, down]) => visibleNames.has(up) && visibleNames.has(down)),
    [effectiveDeps, visibleNames],
  );

  const handleNodeClick = useCallback((taskName: string) => {
    if (isLargeGraph && !expanded) {
      // In collapsed mode: only toggle focus to expand downstream in-place
      setFocusedNode((prev) => prev === taskName ? null : taskName);
      return; // Don't navigate away
    }
    // In expanded / small-graph mode, forward the click to the parent handler
    onTaskClick?.(taskName);
  }, [isLargeGraph, expanded, onTaskClick]);

  if (tasks.length === 0) {
    return (
      <div className="flex items-center justify-center rounded-xl border border-dashed border-gray-300 py-16 text-sm text-gray-400 dark:border-gray-700">
        No tasks defined for this DAG.
      </div>
    );
  }

  const taskNames = visibleTasks.map((t) => t.name);
  const layerMap = assignLayers(taskNames, visibleDeps);
  const maxLayer = Math.max(0, ...Object.values(layerMap));

  const byLayer: Record<number, string[]> = {};
  taskNames.forEach((name) => {
    const l = layerMap[name] ?? 0;
    if (!byLayer[l]) byLayer[l] = [];
    byLayer[l].push(name);
  });

  const maxInLayer = Math.max(1, ...Object.values(byLayer).map((a) => a.length));

  const pos: Record<string, { x: number; y: number }> = {};
  Object.entries(byLayer).forEach(([lStr, names]) => {
    const l = Number(lStr);
    const count = names.length;
    const totalH = count * NODE_H + (count - 1) * (NODE_GAP_Y - NODE_H);
    const maxH = maxInLayer * NODE_H + (maxInLayer - 1) * (NODE_GAP_Y - NODE_H);
    const topOffset = (maxH - totalH) / 2;
    names.forEach((name, i) => {
      pos[name] = {
        x: PADDING + l * (NODE_W + LAYER_GAP_X),
        y: PADDING + topOffset + i * NODE_GAP_Y,
      };
    });
  });

  const svgWidth = PADDING * 2 + (maxLayer + 1) * (NODE_W + LAYER_GAP_X) - LAYER_GAP_X + NODE_W;
  const svgHeight = PADDING * 2 + maxInLayer * NODE_GAP_Y;

  const actualHeight = Math.max(svgHeight, 120);

  return (
    <div>
      <div className="overflow-auto rounded-xl border border-gray-200 bg-white dark:border-gray-800 dark:bg-gray-900" style={{ maxHeight: '600px' }}>
        <svg
          viewBox={`0 0 ${svgWidth} ${actualHeight}`}
          preserveAspectRatio="xMinYMid meet"
          style={{ display: 'block', width: '100%', height: 'auto', minWidth: Math.min(svgWidth, 700) }}
        >
        <defs>
          <marker id="dag-arrow" markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto">
            <path d="M0,0.5 L0,6.5 L6.5,3.5 z" fill="#9ca3af" />
          </marker>
        </defs>

        {/* Edges */}
        {visibleDeps.map(([up, down], i) => {
          const src = pos[up];
          const dst = pos[down];
          if (!src || !dst) return null;
          const sx = src.x + NODE_W;
          const sy = src.y + NODE_H / 2;
          const ex = dst.x;
          const ey = dst.y + NODE_H / 2;
          const mx = (sx + ex) / 2;
          return (
            <path
              key={`edge-${i}`}
              d={`M ${sx},${sy} C ${mx},${sy} ${mx},${ey} ${ex},${ey}`}
              fill="none"
              stroke="#d1d5db"
              strokeWidth="1.5"
              markerEnd="url(#dag-arrow)"
            />
          );
        })}

        {/* Nodes */}
        {visibleTasks.map((task) => {
          const p = pos[task.name];
          if (!p) return null;
          const state = instancesByTask[task.name];
          const isFocused = focusedNode === task.name;
          const accentColor = state
            ? (STATE_COLORS[state.toLowerCase()] ?? '#6366f1')
            : (TYPE_COLORS[task.task_type?.toLowerCase() ?? 'default'] ?? TYPE_COLORS.default);
          const label = task.name.length > 20 ? task.name.slice(0, 18) + '…' : task.name;
          const subLabel = state ? state.toUpperCase() : (task.task_type ?? 'task');
          // Show downstream count hint in collapsed mode
          const downCount = isLargeGraph && !expanded
            ? getDownstream(task.name, effectiveDeps).size
            : 0;

          return (
            <g
              key={task.name}
              onClick={() => handleNodeClick(task.name)}
              style={{ cursor: 'pointer' }}
            >
              {/* Focus ring */}
              {isFocused && (
                <rect
                  x={p.x - 4}
                  y={p.y - 4}
                  width={NODE_W + 8}
                  height={NODE_H + 8}
                  rx={12}
                  fill="none"
                  stroke="#6366f1"
                  strokeWidth="2"
                  strokeDasharray="4 2"
                  opacity="0.7"
                />
              )}
              {/* Shadow / glow for state */}
              {state && (
                <rect
                  x={p.x - 2}
                  y={p.y - 2}
                  width={NODE_W + 4}
                  height={NODE_H + 4}
                  rx={10}
                  fill="none"
                  stroke={accentColor}
                  strokeWidth="1"
                  opacity="0.3"
                />
              )}
              {/* Card */}
              <rect
                x={p.x}
                y={p.y}
                width={NODE_W}
                height={NODE_H}
                rx={8}
                fill="white"
                stroke={accentColor}
                strokeWidth={state ? 2 : 1.5}
              />
              {/* Left color bar */}
              <rect
                x={p.x}
                y={p.y}
                width={5}
                height={NODE_H}
                rx={8}
                fill={accentColor}
              />
              {/* Mask the right half of the color bar's radius */}
              <rect
                x={p.x + 3}
                y={p.y}
                width={4}
                height={NODE_H}
                fill={accentColor}
              />
              {/* Task name */}
              <text
                x={p.x + 14}
                y={p.y + NODE_H / 2 - 5}
                fontSize="11"
                fontWeight="600"
                fill="#111827"
              >
                {label}
              </text>
              {/* Sub-label */}
              <text
                x={p.x + 14}
                y={p.y + NODE_H / 2 + 10}
                fontSize="9.5"
                fill={state ? accentColor : '#6b7280'}
                fontWeight="500"
              >
                {subLabel}
              </text>
              {/* Downstream count badge */}
              {downCount > 0 && !focusedNode && (
                <g>
                  <circle
                    cx={p.x + NODE_W - 6}
                    cy={p.y + 6}
                    r={9}
                    fill="#6366f1"
                  />
                  <text
                    x={p.x + NODE_W - 6}
                    y={p.y + 10}
                    fontSize="8"
                    fontWeight="700"
                    fill="white"
                    textAnchor="middle"
                  >
                    {downCount > 99 ? '99+' : downCount}
                  </text>
                </g>
              )}
            </g>
          );
        })}
      </svg>
      </div>

      {/* Controls bar — outside scroll container for easy access */}
      {isLargeGraph && (
        <div className="flex items-center justify-center gap-3 rounded-b-xl border border-t-0 border-gray-200 bg-gray-50/80 px-4 py-3 dark:border-gray-800 dark:bg-gray-800/50">
          {focusedNode && !expanded && (
            <button
              onClick={() => setFocusedNode(null)}
              className="inline-flex items-center gap-1.5 rounded-lg bg-indigo-100 px-3 py-1.5 text-xs font-medium text-indigo-700 transition-colors hover:bg-indigo-200 dark:bg-indigo-900/30 dark:text-indigo-300 dark:hover:bg-indigo-900/50"
            >
              ← Back to first {MAX_VISIBLE_NODES} nodes
            </button>
          )}
          {!expanded ? (
            <button
              onClick={() => { setExpanded(true); setFocusedNode(null); }}
              className="inline-flex items-center gap-1.5 rounded-lg bg-gray-100 px-3 py-1.5 text-xs font-medium text-gray-700 transition-colors hover:bg-gray-200 dark:bg-gray-800 dark:text-gray-300 dark:hover:bg-gray-700"
            >
              {focusedNode
                ? `Showing ${visibleTasks.length} downstream nodes — Expand all ${totalNodes} nodes`
                : `Showing ${visibleTasks.length} of ${totalNodes} nodes — Expand all`}
            </button>
          ) : (
            <button
              onClick={() => { setExpanded(false); setFocusedNode(null); }}
              className="inline-flex items-center gap-1.5 rounded-lg bg-gray-100 px-3 py-1.5 text-xs font-medium text-gray-700 transition-colors hover:bg-gray-200 dark:bg-gray-800 dark:text-gray-300 dark:hover:bg-gray-700"
            >
              Showing all {totalNodes} nodes — Collapse
            </button>
          )}
        </div>
      )}
    </div>
  );
}

/** Derive a per-task state map from a list of TaskInstances for a given run */
export function instanceStateForRun(instances: TaskInstance[], runId: string): Record<string, string> {
  const map: Record<string, string> = {};
  for (const inst of instances) {
    if (inst.run_id === runId) {
      // task_id in instances is the DB uuid; we need to map to task name
      // We'll store by task_id and callers can resolve — OR the caller must
      // pass a taskById map. We expose a helper that takes task list too.
      map[inst.task_id] = inst.state;
    }
  }
  return map;
}

/** Build task_id → task_name map then derive state by task_name */
export function instanceStateByName(
  instances: TaskInstance[],
  tasks: DagTask[],
  runId?: string,
): Record<string, string> {
  const idToName: Record<string, string> = {};
  tasks.forEach((t) => { idToName[t.id] = t.name; });
  const map: Record<string, string> = {};
  for (const inst of instances) {
    if (!runId || inst.run_id === runId) {
      const name = idToName[inst.task_id] ?? inst.task_id;
      map[name] = inst.state;
    }
  }
  return map;
}
