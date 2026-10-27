"use strict";
// The browser owns selection and layout. Rust owns parsing, validation and persistence.
const $ = (s) => document.querySelector(s),
  DAY = 86400000;
const VIEWS = [
  ["board", "カンバン", "task"],
  ["gantt", "ガント", "task"],
  ["tasks", "タスク一覧", "task"],
  ["issues", "課題一覧", "issue"],
  ["network", "依存ネットワーク", null],
  ["down", "バーンダウン", "task"],
  ["up", "バーンアップ", "task"],
  ["masters", "マスタ", null],
  ["bugs", "バグ一覧", "bug"],
  ["bugstats", "バグ統計", "bug"],
  ["issuestats", "課題統計", "issue"],
  ["inquiries", "依頼・質問一覧", "inquiry"],
  ["workarounds", "暫定対応一覧", "workaround"],
  ["inquirystats", "依頼・質問統計", "inquiry"],
  ["workaroundstats", "暫定対応統計", "workaround"],
];
const S = {
  csrf: "",
  user: "",
  readonly: false,
  meta: null,
  p: null,
  data: null,
  view: loadLayout().view || "board",
  master: "category",
  filter: {},
  filters: {},
  rows: [],
  cols: [],
  anchor: [0, 0],
  current: [0, 0],
  selected: new Set(),
  selecting: false,
  editor: null,
  load: 0,
  focus: null,
  range: null,
  layout: loadLayout(),
  drag: null,
};
function loadLayout() {
  try {
    return JSON.parse(localStorage.getItem("yyproject-layout") || "{}");
  } catch {
    return {};
  }
}
function persistLayout() {
  try {
    localStorage.setItem("yyproject-layout", JSON.stringify(S.layout));
  } catch {
    /* Layout persistence is optional. */
  }
}
function el(tag, attrs = {}, children = []) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "text") e.textContent = v;
    else if (k === "class") e.className = v;
    else if (k.startsWith("on")) e.addEventListener(k.slice(2), v);
    else if (v !== false && v != null)
      e.setAttribute(k, v === true ? (k === "draggable" ? "true" : "") : v);
  }
  for (const c of [children].flat())
    if (c != null)
      e.append(typeof c === "string" ? document.createTextNode(c) : c);
  return e;
}
function svg(tag, attrs = {}, text) {
  const e = document.createElementNS("http://www.w3.org/2000/svg", tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  if (text != null) e.textContent = text;
  return e;
}
function message(text, success = false) {
  const e = $("#message");
  e.hidden = !text;
  e.textContent = text;
  e.classList.toggle("success", success);
}
function report(error) {
  message(
    error.status === 409
      ? "他の操作で更新されています。入力を確認し、「再読込」してから再編集してください。\n" +
          error.message
      : error.message || String(error),
  );
}
function guarded(f) {
  return async (...args) => {
    try {
      return await f(...args);
    } catch (e) {
      report(e);
    }
  };
}
async function request(path, body, method = "POST") {
  const response = await fetch(path, {
    method,
    credentials: "same-origin",
    headers:
      method === "POST"
        ? { "Content-Type": "application/json", "X-YY-CSRF": S.csrf }
        : {},
    body: method === "POST" ? JSON.stringify(body) : undefined,
  });
  const data = await response.json();
  if (!response.ok) {
    if (response.status === 401 && path != "/web/login") {
      showLogin();
      message("セッションが終了しました。ログインし直してください。");
    }
    const e = new Error(data.error || "操作に失敗しました");
    e.status = response.status;
    throw e;
  }
  return data;
}
async function api(v) {
  const writing =
    ["grid", "edit", "edit_master"].includes(v.action) ||
    (v.action === "call" &&
      ["project_create", "project_change"].includes(v.name));
  if (writing) S.mutating = (S.mutating || 0) + 1;
  try {
    return await request("/web/api", v);
  } finally {
    if (writing) S.mutating--;
  }
}
const call = (name, args = {}) => api({ action: "call", name, args });
function showLogin() {
  S.load++;
  S.editor = null;
  document.querySelectorAll("dialog").forEach((d) => d.remove());
  $("#context-menu").hidden = true;
  $("#app").hidden = true;
  $("#login").hidden = false;
  S.csrf = "";
  S.p = null;
  S.data = null;
  $("#surface").replaceChildren();
  $("#password").focus();
}
async function authenticated(u) {
  S.csrf = u.csrf;
  S.user = u.user;
  S.readonly = u.read_only;
  applyReadOnly();
  $("#password").value = "";
  $("#login-error").textContent = "";
  $("#login").hidden = true;
  $("#app").hidden = false;
  $("#account").textContent = u.user + (u.read_only ? "（参照）" : "");
  S.meta = await api({ action: "metadata" });
  await projects();
}
async function projects(selected) {
  const result = await call("project_list");
  const select = $("#projects");
  select.replaceChildren(
    ...result.projects.map((p) => el("option", { value: p.id, text: p.name })),
  );
  const wanted = selected || S.layout.project;
  if (wanted && [...select.options].some((o) => o.value === String(wanted)))
    select.value = wanted;
  if (!select.value) {
    S.p = null;
    $("#surface").replaceChildren(
      el("div", {
        class: "empty",
        text: "「新規」からプロジェクトを作成してください",
      }),
    );
    return;
  }
  await reload();
}
function view() {
  return VIEWS.find((v) => v[0] === S.view) || VIEWS[0];
}
function kind() {
  return view()[2] || "task";
}
function isGrid() {
  return [
    "tasks",
    "issues",
    "bugs",
    "inquiries",
    "workarounds",
    "masters",
  ].includes(S.view);
}
function isStats() {
  return S.view.endsWith("stats");
}
function writable() {
  if (S.readonly) throw new Error("読み取り専用ユーザーです");
  if (!S.p) throw new Error("プロジェクトを選択してください");
}
function pArgs() {
  return { project_id: S.p.id, expected_revision: S.p.revision };
}
function task(id) {
  return S.p.tasks.find((t) => t.id === Number(id));
}
function selectedIds() {
  return S.rows
    .filter((r, i) =>
      S.selected.size
        ? S.selected.has(r.id)
        : i >= Math.min(S.anchor[0], S.current[0]) &&
          i <= Math.max(S.anchor[0], S.current[0]),
    )
    .map((r) => r.id);
}
function focusedId() {
  return isGrid() ? S.rows[S.current[0]]?.id : S.focus;
}
async function change(c, revision = S.p.revision) {
  writable();
  message("更新しています…");
  await call("project_change", {
    project_id: S.p.id,
    expected_revision: revision,
    change: c,
  });
  await reload(false);
  message("保存しました", true);
}
async function reload(reset = true) {
  if (S.editor)
    throw new Error("編集中のセルを確定するかEscで取り消してください");
  const scroll = reset
    ? null
    : [$("#surface").scrollLeft, $("#surface").scrollTop];
  const token = ++S.load;
  const data = await api({
    action: "project",
    project_id: Number($("#projects").value),
    filter: S.view === "masters" ? {} : S.filter,
  });
  if (token !== S.load) return;
  S.data = data;
  S.p = data.project;
  if (reset) {
    S.anchor = [0, 0];
    S.current = [0, 0];
    S.selected.clear();
    S.focus = null;
  }
  $("#revision").textContent = `版 ${S.p.revision} · ${S.p.tasks.length}項目`;
  $("#view-title").textContent = view()[1];
  $("#search").value = S.filter.query || "";
  $("#archived").checked = !!S.filter.archived;
  message("");
  await render();
  if (scroll) {
    $("#surface").scrollLeft = scroll[0];
    $("#surface").scrollTop = scroll[1];
  }
}
async function switchView(name) {
  if (S.editor)
    throw new Error("編集中のセルを確定するかEscで取り消してください");
  S.filters[S.view] = structuredClone(S.filter);
  S.view = name;
  S.layout.view = name;
  persistLayout();
  S.filter = structuredClone(S.filters[name] || {});
  $("#views")
    .querySelectorAll("button")
    .forEach((b) =>
      b.setAttribute("aria-current", String(b.dataset.view === name)),
    );
  await reload();
}
function applyReadOnly() {
  for (const id of [
    "add",
    "edit",
    "bulk",
    "paste",
    "fill",
    "archive",
    "restore",
    "exchange",
    "dependency",
    "schedule",
    "capture",
    "rename-project",
    "new-project",
  ])
    $("#" + id).disabled = S.readonly;
}
function setupButtons() {
  const grid = isGrid(),
    masters = S.view === "masters";
  for (const id of ["bulk", "copy", "paste", "fill"])
    $("#" + id).hidden = !grid;
  $("#exchange").hidden = kind() !== "inquiry";
  $("#jira").hidden = masters;
  $("#restore").hidden = !S.filter.archived;
  $("#archive").hidden = !!S.filter.archived;
  $("#settings").hidden = !(
    ["gantt", "down", "up"].includes(S.view) || isStats()
  );
  $("#subtabs").hidden = !masters;
  applyReadOnly();
  $("#surface").setAttribute("aria-label", view()[1]);
  $("#selection-info").textContent = S.p
    ? `${S.rows.length}行 / ${S.p.name}`
    : "";
}
function foldedTasks() {
  return new Set(S.layout[`outline:${S.p.id}`] || []);
}
function childCount(id) {
  const parent = task(id);
  return S.p.tasks.filter(
    (t) => t.parent === id && t.archived === parent.archived,
  ).length;
}
function outlineRows(rows) {
  const folded = foldedTasks(),
    included = new Set(rows.map((r) => r.id)),
    byId = new Map(S.p.tasks.map((t) => [t.id, t]));
  return rows.filter((r) => {
    let parent = byId.get(r.id)?.parent;
    const seen = new Set();
    while (parent != null && !seen.has(parent)) {
      seen.add(parent);
      if (included.has(parent) && folded.has(parent)) return false;
      parent = byId.get(parent)?.parent;
    }
    return true;
  });
}
async function toggleOutline(id) {
  if (S.editor)
    throw new Error("編集中のセルを確定するかEscで取り消してください");
  const folded = foldedTasks();
  folded.has(id) ? folded.delete(id) : folded.add(id);
  S.layout[`outline:${S.p.id}`] = [...folded];
  persistLayout();
  const surface = $("#surface"),
    x = surface.scrollLeft,
    y = surface.scrollTop;
  S.selected.clear();
  S.selecting = false;
  S.focus = id;
  await render();
  const index = S.rows.findIndex((r) => r.id === id);
  S.anchor = S.current = [
    Math.max(0, index),
    Math.max(
      0,
      S.cols.findIndex((c) => c.key === "title"),
    ),
  ];
  if (isGrid()) selectionPaint();
  surface.scrollLeft = x;
  surface.scrollTop = y;
  surface.focus({ preventScroll: true });
}
function outlineTitle(record) {
  const id = record.id,
    t = task(id),
    children = childCount(id),
    folded = foldedTasks().has(id),
    depth = Math.min(record.depth || 0, 12);
  const title = el("div", { class: "outline-title" });
  const guides = el("span", {
    class: "outline-guides",
    "aria-hidden": "true",
    hidden: depth === 0,
  });
  guides.style.width = depth * 18 + "px";
  title.append(guides);
  if (children)
    title.append(
      el("button", {
        type: "button",
        class: "outline-toggle",
        "data-outline-id": id,
        "aria-label": `タスク#${id}の子タスク`,
        "aria-expanded": String(!folded),
        title: `${folded ? "展開する" : "折りたたむ"}（子${children}件）`,
        text: folded ? "▶" : "▼",
        onpointerdown: (e) => e.stopPropagation(),
        onclick: guarded(async (e) => {
          e.stopPropagation();
          await toggleOutline(id);
        }),
        ondblclick: (e) => e.stopPropagation(),
      }),
    );
  else
    title.append(
      el("span", {
        class: "outline-leaf",
        "aria-hidden": "true",
        text: depth ? "•" : "",
      }),
    );
  title.append(
    el("span", {
      class: "outline-subject",
      text: (t.milestone ? "◆ " : "") + t.title,
    }),
  );
  if (children)
    title.append(
      el("span", { class: "outline-count", text: `子${children}件` }),
    );
  if (t.parent != null)
    title.append(
      el("span", {
        class: "outline-parent",
        text: `親 #${t.parent}`,
        title: task(t.parent)?.title || "",
      }),
    );
  return title;
}

async function render() {
  const surface = $("#surface");
  surface.replaceChildren();
  S.rows =
    S.view === "masters"
      ? S.data.master_rows.filter(
          (r) =>
            S.p.masters.find((m) => m.id === r.id)?.kind === S.master &&
            Object.entries(S.filter.columns || {}).every(
              ([k, v]) => r.values[k] === v,
            ) &&
            Object.values(r.values).some((v) =>
              String(v)
                .toLowerCase()
                .includes((S.filter.query || "").toLowerCase()),
            ),
        )
      : S.data.rows.filter((r) => !view()[2] || task(r.id).kind === kind());
  if (["tasks", "gantt"].includes(S.view)) S.rows = outlineRows(S.rows);
  S.cols =
    S.view === "masters" ? S.meta.masters[S.master] : S.meta.columns[kind()];
  setupButtons();
  if (isGrid()) renderGrid();
  else if (S.view === "board") renderBoard();
  else if (S.view === "gantt") renderGantt();
  else if (S.view === "network") renderNetwork();
  else await renderStatistics();
}
function width(c) {
  return S.layout[`column:${S.view}:${S.master}:${c.key}`] || c.width;
}
function height(id) {
  return (
    S.layout[`row:${S.p.id}:${S.view}:${id}`] || (S.view === "gantt" ? 52 : 36)
  );
}
function bounds() {
  return [
    Math.min(S.anchor[0], S.current[0]),
    Math.max(S.anchor[0], S.current[0]),
    Math.min(S.anchor[1], S.current[1]),
    Math.max(S.anchor[1], S.current[1]),
  ];
}
function resize(event, apply, done) {
  if (event.button !== 0) return;
  event.preventDefault();
  event.stopPropagation();
  const target = event.currentTarget;
  target.setPointerCapture(event.pointerId);
  const move = (e) =>
    apply(e.clientX - event.clientX, e.clientY - event.clientY);
  let active = true;
  const end = (e, cancel = false) => {
    if (!active) return;
    active = false;
    if (cancel) apply(0, 0);
    if (target.hasPointerCapture(event.pointerId))
      target.releasePointerCapture(event.pointerId);
    target.removeEventListener("pointermove", move);
    target.removeEventListener("pointerup", end);
    target.removeEventListener("pointercancel", cancelDrag);
    target.removeEventListener("lostpointercapture", cancelDrag);
    document.removeEventListener("keydown", key);
    if (!cancel) persistLayout();
    done();
  };
  const cancelDrag = (e) => end(e, true);
  const key = (e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      end(e, true);
    }
  };
  target.addEventListener("pointermove", move);
  target.addEventListener("pointerup", end);
  target.addEventListener("pointercancel", cancelDrag);
  target.addEventListener("lostpointercapture", cancelDrag);
  document.addEventListener("keydown", key);
}
let gridBody = null,
  gridRange = "",
  gridOffsets = [],
  gridTable = null;
function renderGrid() {
  const surface = $("#surface");
  const table = el("table", {
    class: "grid",
    role: S.view === "tasks" ? "treegrid" : "grid",
    "aria-label": view()[1],
  });
  gridTable = table;
  const cg = el("colgroup", {}, [
    el("col", { width: 34 }),
    ...S.cols.map((c) => el("col", { width: width(c) })),
  ]);
  table.append(cg);
  table.style.width = 34 + S.cols.reduce((n, c) => n + width(c), 0) + "px";
  const head = el("tr", {}, [
    el(
      "th",
      { class: "frozen" },
      el("input", {
        type: "checkbox",
        "aria-label": "表示行をすべて選択",
        onchange: (e) => {
          S.selected = e.target.checked
            ? new Set(S.rows.map((r) => r.id))
            : new Set();
          selectionPaint();
        },
      }),
    ),
  ]);
  S.cols.forEach((c, col) => {
    const th = el("th", {
      "data-col": col,
      class: col === 0 ? "frozen id-cell" : "",
      oncontextmenu: (e) => {
        e.preventDefault();
        columnMenu(e, c);
      },
    });
    th.append(
      el("div", {
        class: "th-label",
        text:
          (col === 0 ? "↕ " : "") +
          c.label +
          (c.editable && !S.meta.bulk.includes(c.key) && S.view !== "masters"
            ? "（個別）"
            : ""),
      }),
    );
    const handle = el("div", {
      class: "col-resize",
      "aria-label": c.label + "の列幅",
      onpointerdown: (e) => {
        const start = width(c);
        resize(
          e,
          (dx) => {
            const w = Math.max(40, Math.min(2000, start + dx));
            S.layout[`column:${S.view}:${S.master}:${c.key}`] = w;
            cg.children[col + 1].width = w;
            table.style.width =
              34 + S.cols.reduce((n, c) => n + width(c), 0) + "px";
          },
          () => renderGridFresh(),
        );
      },
    });
    th.append(handle);
    head.append(th);
  });
  table.append(el("thead", {}, head));
  gridBody = el("tbody");
  table.append(gridBody);
  surface.append(table);
  gridRange = "";
  gridOffsets = [0];
  for (const r of S.rows) gridOffsets.push(gridOffsets.at(-1) + height(r.id));
  drawGridRows();
  surface.onscroll = () => {
    if (!S.editor) requestAnimationFrame(drawGridRows);
  };
  if (S.view === "masters") {
    $("#subtabs").replaceChildren(
      ...[
        ["category", "カテゴリ"],
        ["assignee", "担当者"],
        ["holiday", "休日"],
      ].map(([k, label]) =>
        el("button", {
          text: label,
          class: k === S.master ? "active" : "",
          onclick: guarded(async () => {
            S.master = k;
            S.current = [0, 0];
            S.anchor = [0, 0];
            S.selected.clear();
            await render();
          }),
        }),
      ),
    );
  }
  surface.focus({ preventScroll: true });
}
function renderGridFresh() {
  const surface = $("#surface"),
    x = surface.scrollLeft,
    y = surface.scrollTop;
  surface.replaceChildren();
  renderGrid();
  surface.scrollLeft = x;
  surface.scrollTop = y;
}
function drawGridRows() {
  if (!gridBody || !isGrid() || S.editor) return;
  const surface = $("#surface");
  let start = 0;
  while (
    start < S.rows.length &&
    gridOffsets[start + 1] < surface.scrollTop - 100
  )
    start++;
  let end = start;
  while (
    end < S.rows.length &&
    gridOffsets[end] < surface.scrollTop + surface.clientHeight + 150
  )
    end++;
  const key = `${start}:${end}`;
  if (key === gridRange) return;
  gridRange = key;
  gridBody.replaceChildren();
  const spacer = (h) =>
    el(
      "tr",
      { "aria-hidden": "true" },
      el("td", { colspan: S.cols.length + 1 }, el("div", {})),
    );
  if (start) {
    const r = spacer();
    r.firstChild.firstChild.style.height = gridOffsets[start] + "px";
    gridBody.append(r);
  }
  for (let row = start; row < end; row++) {
    const record = S.rows[row],
      id = record.id,
      h = height(id),
      tr = el("tr", {
        "data-id": id,
        ...(S.view === "tasks"
          ? { "aria-level": (record.depth || 0) + 1 }
          : {}),
      });
    const checkbox = el("input", {
      type: "checkbox",
      "aria-label": `行${id}を選択`,
      onchange: (e) => {
        if (e.target.checked) S.selected.add(id);
        else S.selected.delete(id);
        selectionPaint();
      },
    });
    checkbox.checked = S.selected.has(id);
    const mark = el("td", { class: "frozen" }, checkbox);
    tr.append(mark);
    S.cols.forEach((c, col) => {
      const td = el("td", {
        "data-row": row,
        "data-col": col,
        role: "gridcell",
        tabindex: -1,
        class:
          (!c.editable ? "readonly " : "") +
          (col === 0 ? "frozen id-cell row-handle" : ""),
        title: record.values[c.key] || "",
        onpointerdown: (e) => {
          if (e.button !== 0 || e.target.closest(".row-resize,.outline-toggle"))
            return;
          if (S.editor) return;
          if (e.ctrlKey || e.metaKey) {
            S.selected.has(id) ? S.selected.delete(id) : S.selected.add(id);
            S.current = [row, col];
          } else {
            S.selected.clear();
            if (!e.shiftKey) S.anchor = [row, col];
            S.current = [row, col];
          }
          S.selecting = col !== 0;
          selectionPaint();
          $("#surface").focus({ preventScroll: true });
        },
        onpointerenter: (e) => {
          if (S.selecting && e.buttons === 1 && !S.editor) {
            S.current = [row, col];
            selectionPaint();
          }
        },
        ondblclick: guarded(() => editCell(row, col)),
        oncontextmenu: (e) => {
          e.preventDefault();
          if (!selectedIds().includes(id)) {
            S.anchor = [row, col];
            S.current = [row, col];
            S.selected.clear();
            selectionPaint();
          }
          rowMenu(e, id);
        },
      });
      td.style.height = h + "px";
      const text = el("div", {
        class: "cell-text",
        text:
          c.key === "title" && S.view === "tasks"
            ? ""
            : record.values[c.key] || "",
      });
      if (c.key === "title" && S.view === "tasks")
        text.append(outlineTitle(record));
      text.style.maxHeight = h - 1 + "px";
      td.append(text);
      if (col === 0) {
        td.draggable = S.view !== "masters" && !S.readonly;
        td.addEventListener("dragstart", (e) => {
          S.drag = { id, type: "row", revision: S.p.revision };
          e.dataTransfer.setData("text/plain", String(id));
          e.dataTransfer.effectAllowed = "move";
          tr.classList.add("dragging");
        });
        td.addEventListener("dragend", () => {
          S.drag = null;
          tr.classList.remove("dragging");
          clearDrop();
        });
        td.append(
          el("div", {
            class: "row-resize",
            "aria-label": `行${id}の高さ`,
            onpointerdown: (e) => {
              resize(
                e,
                (_, dy) => {
                  S.layout[`row:${S.p.id}:${S.view}:${id}`] = Math.max(
                    32,
                    Math.min(500, h + dy),
                  );
                  for (const cell of tr.children) {
                    cell.style.height = height(id) + "px";
                    if (cell.firstChild?.classList?.contains("cell-text"))
                      cell.firstChild.style.maxHeight = height(id) - 1 + "px";
                  }
                },
                renderGridFresh,
              );
            },
          }),
        );
      }
      tr.append(td);
    });
    tr.addEventListener("dragover", (e) => {
      if (S.drag?.type === "row" && S.drag.id !== id) {
        e.preventDefault();
        clearDrop();
        tr.classList.add("drop-target");
        edgeScroll(e);
      }
    });
    tr.addEventListener(
      "drop",
      guarded(async (e) => {
        e.preventDefault();
        const d = S.drag;
        S.drag = null;
        clearDrop();
        if (d && d.id !== id)
          await change(
            { operation: "move_task", id: d.id, before: id },
            d.revision,
          );
      }),
    );
    gridBody.append(tr);
  }
  if (end < S.rows.length) {
    const r = spacer();
    r.firstChild.firstChild.style.height =
      gridOffsets.at(-1) - gridOffsets[end] + "px";
    gridBody.append(r);
  }
  selectionPaint();
}
function selectionPaint() {
  const [r0, r1, c0, c1] = bounds();
  $("#surface")
    .querySelectorAll("td[data-row]")
    .forEach((e) => {
      const r = Number(e.dataset.row),
        c = Number(e.dataset.col);
      e.classList.toggle(
        "selected",
        !S.selected.size && r >= r0 && r <= r1 && c >= c0 && c <= c1,
      );
      e.classList.toggle("current", r === S.current[0] && c === S.current[1]);
      e.classList.toggle("row-selected", S.selected.has(S.rows[r]?.id));
      e.setAttribute(
        "aria-selected",
        String(
          S.selected.has(S.rows[r]?.id) ||
            (!S.selected.size && r >= r0 && r <= r1 && c >= c0 && c <= c1),
        ),
      );
    });
  $("#surface")
    .querySelectorAll("tr[data-id] input[type=checkbox]")
    .forEach(
      (e) => (e.checked = S.selected.has(Number(e.closest("tr").dataset.id))),
    );
  $("#selection-info").textContent =
    `${S.rows.length}行 · ${selectedIds().length}行選択 · ${S.p?.name || ""}`;
}
function edgeScroll(e) {
  const box = $("#surface").getBoundingClientRect();
  if (e.clientY > box.bottom - 40) $("#surface").scrollTop += 22;
  if (e.clientY < box.top + 40) $("#surface").scrollTop -= 22;
  if (e.clientX > box.right - 35) $("#surface").scrollLeft += 22;
  if (e.clientX < box.left + 35) $("#surface").scrollLeft -= 22;
}
function clearDrop() {
  document
    .querySelectorAll(".drop-target,.drag-over")
    .forEach((e) => e.classList.remove("drop-target", "drag-over"));
}
function encode(rows, sep = "\t") {
  return (
    rows
      .map((r) =>
        r
          .map((v) => {
            v = String(v ?? "");
            return v.includes(sep) || /["\r\n]/.test(v)
              ? '"' + v.replaceAll('"', '""') + '"'
              : v;
          })
          .join(sep),
      )
      .join("\r\n") + "\r\n"
  );
}
function decode(text) {
  const rows = [];
  let row = [],
    value = "",
    quoted = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === '"') {
      if (quoted && text[i + 1] === '"') {
        value += '"';
        i++;
      } else if (quoted || !value) quoted = !quoted;
      else value += c;
    } else if (!quoted && c === "\t") {
      row.push(value);
      value = "";
    } else if (!quoted && (c === "\r" || c === "\n")) {
      if (c === "\r" && text[i + 1] === "\n") i++;
      row.push(value);
      rows.push(row);
      row = [];
      value = "";
    } else value += c;
  }
  if (quoted) throw new Error("引用符が閉じていません");
  if (value || row.length) {
    row.push(value);
    rows.push(row);
  }
  return rows;
}
function selectionText() {
  const [r0, r1, c0, c1] = bounds();
  const records = S.selected.size
    ? S.rows.filter((r) => S.selected.has(r.id))
    : S.rows.slice(r0, r1 + 1);
  return encode(
    records.map((r) =>
      S.cols.slice(c0, c1 + 1).map((c) => r.values[c.key] || ""),
    ),
  );
}
async function sendGrid(
  ids,
  keys,
  text,
  fill_down = false,
  revision = S.p.revision,
) {
  writable();
  message("更新しています…");
  await api({
    action: "grid",
    edit: {
      project_id: S.p.id,
      expected_revision: revision,
      ids,
      keys,
      text,
      master: S.view === "masters" ? S.master : null,
      fill_down,
    },
  });
  await reload(false);
  message(`${ids.length}行を保存しました`, true);
}
async function pasteText(text) {
  if (text === "") text = "\r\n";
  const matrix = decode(text);
  if (!matrix.length) return;
  const [r0, r1, c0, c1] = bounds();
  let ids, keys;
  if (S.selected.size) {
    ids = selectedIds();
    keys = S.cols.slice(c0, c1 + 1).map((c) => c.key);
  } else {
    const one = matrix.length === 1 && matrix[0].length === 1;
    ids = S.rows.slice(r0, one ? r1 + 1 : r0 + matrix.length).map((r) => r.id);
    keys = S.cols
      .slice(c0, one ? c1 + 1 : c0 + matrix[0].length)
      .map((c) => c.key);
    if (
      ids.length !== (one ? r1 - r0 + 1 : matrix.length) ||
      (!one && keys.length !== matrix[0].length)
    )
      throw new Error("貼り付け範囲が一覧を超えています");
  }
  await sendGrid(ids, keys, text);
}
async function fillDown() {
  const ids = selectedIds(),
    [, , c0, c1] = bounds();
  if (ids.length < 2) throw new Error("同じ列の2行以上を選択してください");
  const first = S.rows.find((r) => r.id === ids[0]);
  const keys = S.cols.slice(c0, c1 + 1).map((c) => c.key);
  await sendGrid(
    ids,
    keys,
    encode([keys.map((k) => first.values[k] || "")]),
    true,
  );
}
function enumOptions(key) {
  if (S.meta.enums[key]) return S.meta.enums[key];
  if (key.endsWith("_difficulty")) return S.meta.enums.difficulty;
  if (key === "inquiry.kind")
    return [
      ["request", "依頼"],
      ["question", "質問"],
    ];
  if (key === "include_in_effort")
    return [
      ["true", "算入"],
      ["false", "除外"],
    ];
  if (key === "milestone")
    return [
      ["true", "はい"],
      ["false", "いいえ"],
    ];
  if (key === "kind")
    return [
      ["task", "タスク"],
      ["issue", "課題"],
      ["bug", "バグ"],
      ["inquiry", "依頼・質問"],
      ["workaround", "暫定対応"],
    ];
  if (["assignee", "reviewer", "category"].includes(key)) {
    const type = key === "category" ? "category" : "assignee";
    return [
      ["", "（未指定）"],
      ...S.p.masters
        .filter((m) => m.kind === type)
        .map((m) => [
          String(m.id),
          type === "category" ? categoryPath(m.id) : m.name,
        ]),
    ];
  }
  return null;
}
function categoryPath(id) {
  const names = [];
  for (let n = 0; id && n < 3; n++) {
    const m = S.p.masters.find((m) => m.id === id);
    if (!m) break;
    names.unshift(m.name);
    id = m.parent;
  }
  return names.join(" / ");
}
function fieldControl(key, value = "", large = false) {
  const options = enumOptions(key);
  let e;
  if (options) {
    e = el(
      "select",
      {},
      options.map(([key, label]) => el("option", { value: key, text: label })),
    );
    const found = options.find(
      ([k, label]) =>
        String(value) === String(k) || String(value) === String(label),
    );
    e.value = found ? found[0] : options[0][0];
  } else {
    const date =
      /^(start|due|completed_on|date)$/.test(key) || key.endsWith("_on");
    e = el(large ? "textarea" : "input", { type: date ? "date" : "text" });
    e.value = value ?? "";
    if (key.endsWith("_at")) e.placeholder = "2026-10-27T09:00:00+09:00";
  }
  return e;
}
async function editCell(row, col, seed) {
  writable();
  if (S.filter.archived) throw new Error("復元してから編集してください");
  const c = S.cols[col],
    record = S.rows[row];
  if (!c.editable) throw new Error("読み取り専用の列です");
  if (S.editor) return;
  const td = $("#surface").querySelector(
    `td[data-row="${row}"][data-col="${col}"]`,
  );
  if (!td) return;
  const revision = S.p.revision,
    e = fieldControl(c.key, seed ?? record.values[c.key], true);
  e.classList.add("cell-editor");
  S.editor = e;
  td.append(e);
  e.focus();
  if (e.select) e.select();
  let saving = false;
  const close = () => {
    e.remove();
    S.editor = null;
    $("#surface").focus({ preventScroll: true });
  };
  const save = async (next) => {
    if (saving) return;
    saving = true;
    try {
      const text = e.value;
      await api({
        action: "grid",
        edit: {
          project_id: S.p.id,
          expected_revision: revision,
          ids: [record.id],
          keys: [c.key],
          text: encode([[text]]),
          master: S.view === "masters" ? S.master : null,
          fill_down: false,
        },
      });
      close();
      await reload(false);
      if (next) {
        S.current = [row, Math.min(col + 1, S.cols.length - 1)];
        S.anchor = [...S.current];
        selectionPaint();
      }
      message("セルを保存しました", true);
    } catch (err) {
      saving = false;
      report(err);
      e.focus();
    }
  };
  e.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    } else if (
      (event.key === "Enter" && !event.shiftKey) ||
      event.key === "Tab"
    ) {
      event.preventDefault();
      void save(event.key === "Tab");
    }
  });
  e.addEventListener("blur", () => {
    if (!saving && S.editor === e) void save(false);
  });
}
function menu(event, items) {
  const m = $("#context-menu");
  m.replaceChildren(
    ...items.map((item) =>
      item
        ? el("button", {
            text: item[0],
            role: "menuitem",
            onclick: guarded(async () => {
              m.hidden = true;
              await item[1]();
            }),
          })
        : el("hr"),
    ),
  );
  m.hidden = false;
  m.style.left = Math.min(event.clientX, innerWidth - m.offsetWidth - 8) + "px";
  m.style.top =
    Math.min(event.clientY, innerHeight - m.offsetHeight - 8) + "px";
}
function columnMenu(event, c) {
  const values = [...new Set(S.rows.map((r) => r.values[c.key] || ""))].sort(
    (a, b) => a.localeCompare(b, "ja"),
  );
  menu(event, [
    [
      "この列の絞り込みを解除",
      async () => {
        delete S.filter.columns?.[c.key];
        await reload();
      },
    ],
    ["条件を入力…", () => filterDialog(c)],
    null,
    ...values.map((value) => [
      value || "（空欄）",
      async () => {
        S.filter.columns ||= {};
        S.filter.columns[c.key] = value;
        await reload();
      },
    ]),
  ]);
}
function rowMenu(event, id) {
  S.focus = id;
  if (S.view === "masters") {
    menu(event, [
      ["マスタを編集", () => masterDialog(id)],
      ["属性を一括変更", bulkDialog],
      ["下方向にコピー", fillDown],
      ["マスタを削除", () => archive(id)],
    ]);
    return;
  }
  const t = task(id);
  const items = [
    ["項目を編集", () => editItem(id)],
    ["選択行の属性を一括変更", bulkDialog],
    ["下方向にコピー", fillDown],
    null,
  ];
  if (t?.kind === "task")
    items.push(["子タスクを追加", () => editItem(null, id)]);
  if (t?.kind === "inquiry")
    items.push(["やりとり記録", () => exchangeDialog(id)]);
  if (S.filter.archived) items.push(["復元", () => restore(id)]);
  else items.push(["保管", () => archive(id)]);
  if (t)
    items.push(
      null,
      ...S.meta.enums.status.map(([s, label]) => [
        `${label}に変更`,
        () => statusChange(id, s),
      ]),
    );
  menu(event, items);
}
function dialog(title, build, onSave, saveLabel = "保存") {
  const d = el("dialog", { "aria-label": title });
  const body = el("div", { class: "dialog-body" }),
    error = el("p", { class: "dialog-error", role: "alert" });
  body.append(error);
  const fields = build(body, d);
  const cancel = el("button", { text: "閉じる", onclick: () => d.close() });
  const actions = el("div", { class: "dialog-actions" }, cancel);
  if (onSave) {
    const save = el("button", {
      text: saveLabel,
      class: "primary",
      onclick: async () => {
        save.disabled = true;
        try {
          await onSave(fields);
          d.close();
        } catch (e) {
          error.textContent =
            e.status === 409
              ? "他から更新されています。再読込後に再編集してください。\n" +
                e.message
              : e.message;
          save.disabled = false;
        }
      },
    });
    actions.append(save);
  }
  d.append(
    el("header", { class: "dialog-head" }, [
      el("h2", { text: title }),
      el("button", {
        text: "×",
        "aria-label": "閉じる",
        onclick: () => d.close(),
      }),
    ]),
    body,
    actions,
  );
  d.addEventListener("close", () => {
    d.remove();
    $("#surface").focus({ preventScroll: true });
  });
  $("#modal-host").append(d);
  d.showModal();
  return d;
}
function formFields(body, fields) {
  const grid = el("div", { class: "form-grid" }),
    controls = {};
  body.append(grid);
  for (const f of fields) {
    if (f.section) {
      grid.append(el("div", { class: "section-label", text: f.section }));
      continue;
    }
    const input = f.options
      ? el(
          "select",
          {},
          f.options.map(([k, label]) =>
            el("option", { value: k, text: label }),
          ),
        )
      : fieldControl(f.key, f.value, !!f.large);
    if (f.options) input.value = f.value ?? f.options[0]?.[0];
    input.id = "field-" + f.key;
    input.setAttribute("aria-label", f.label);
    if (f.readonly) input.disabled = true;
    const label = el(
      "label",
      { class: "form-field" + (f.large ? " wide" : "") },
      [el("span", { text: f.label }), input],
    );
    grid.append(label);
    controls[f.key] = input;
  }
  return controls;
}
function values(controls) {
  return Object.fromEntries(
    Object.entries(controls)
      .filter(([, e]) => !e.disabled)
      .map(([k, e]) => [k, e.value]),
  );
}
async function editItem(id = focusedId(), parent = null) {
  writable();
  if (S.view === "masters") return masterDialog(id);
  const existing = id ? task(id) : null;
  const k = existing?.kind || (parent ? "task" : kind()),
    revision = S.p.revision;
  const row = existing
    ? S.data.rows.find((r) => r.id === id) || { values: {} }
    : await api({ action: "draft", project_id: S.p.id, kind: k });
  let columns = S.meta.columns[k].filter((c) => c.editable);
  dialog(
    existing ? "項目を編集" : parent ? "子タスクを追加" : "項目を追加",
    (body) => {
      const controls = formFields(
        body,
        columns.map((c) => ({
          key: c.key,
          label: c.label,
          value:
            c.key === "parent" && parent != null
              ? String(parent)
              : row.values[c.key] || "",
          large: /description|note|reason|requirements|action|plan/.test(c.key),
        })),
      );
      if (controls.milestone) {
        controls.milestone.addEventListener("change", () => {
          if (controls.milestone.value === "true") {
            controls.duration_days.value = "0";
            controls.estimate.value = "0";
            controls.start.value ||= today();
            controls.due.value = controls.start.value;
          } else if (controls.duration_days.value === "0")
            controls.duration_days.value = "1";
        });
        controls.start.addEventListener("change", () => {
          if (controls.milestone.value === "true")
            controls.due.value = controls.start.value;
        });
      }
      return controls;
    },
    async (controls) => {
      await api({
        action: "edit",
        project_id: S.p.id,
        expected_revision: revision,
        id: existing?.id || 0,
        kind: k,
        parent,
        values: values(controls),
      });
      if (!existing) {
        const parentId = Number(controls.parent?.value || parent);
        if (parentId) {
          const folded = foldedTasks();
          folded.delete(parentId);
          S.layout[`outline:${S.p.id}`] = [...folded];
          persistLayout();
        }
      }
      await reload(false);
      message("項目を保存しました", true);
    },
  );
}
function masterDialog(id) {
  const m = id ? S.p.masters.find((m) => m.id === id) : null;
  const revision = S.p.revision;
  const row = S.data.master_rows.find((r) => r.id === id);
  dialog(
    m ? "マスタを編集" : "マスタを追加",
    (body) =>
      formFields(
        body,
        S.meta.masters[S.master]
          .filter((c) => c.editable)
          .map((c) => ({
            key: c.key,
            label: c.label,
            value: row?.values[c.key] || "",
          })),
      ),
    async (fields) => {
      await api({
        action: "edit_master",
        project_id: S.p.id,
        expected_revision: revision,
        id: m?.id || 0,
        kind: S.master,
        values: values(fields),
      });
      await reload(false);
    },
  );
}
function projectDialog(rename = false) {
  if (S.readonly) throw new Error("読み取り専用です");
  const p = S.p,
    rev = p?.revision;
  dialog(
    rename ? "案件設定" : "プロジェクトを作成",
    (body) =>
      formFields(body, [
        { key: "name", label: "プロジェクト名", value: rename ? p.name : "" },
        { key: "start", label: "開始日", value: rename ? p.start : today() },
        {
          key: "finish",
          label: "完了予定日",
          value: rename ? p.finish : addDay(today(), 30),
        },
      ]),
    async (f) => {
      const v = values(f);
      if (rename) {
        await call("project_change", {
          project_id: p.id,
          expected_revision: rev,
          change: { operation: "rename", ...v },
        });
        await projects(p.id);
      } else {
        const created = await call("project_create", v);
        await projects(created.id);
      }
    },
  );
}
function bulkDialog() {
  writable();
  const ids = selectedIds();
  if (!ids.length) throw new Error("対象の行を選択してください");
  const rev = S.p.revision;
  const cols = S.cols.filter(
    (c) => c.editable && (S.view === "masters" || S.meta.bulk.includes(c.key)),
  );
  dialog(
    `選択行の属性を一括変更（${ids.length}行）`,
    (body) => {
      const fields = [];
      for (const c of cols) {
        const mode = el(
          "select",
          { "aria-label": c.label + "の変更" },
          [
            ["keep", "変更しない"],
            ["set", "設定する"],
            ["clear", "空欄にする"],
          ].map(([k, t]) => el("option", { value: k, text: t })),
        );
        const input = fieldControl(c.key, "");
        input.setAttribute("aria-label", c.label);
        body.append(
          el("div", { class: "bulk-field" }, [
            el("label", { text: c.label }),
            mode,
            input,
          ]),
        );
        fields.push({ c, mode, input });
      }
      return fields;
    },
    async (fields) => {
      const chosen = fields.filter((f) => f.mode.value !== "keep");
      if (!chosen.length) return;
      await sendGrid(
        ids,
        chosen.map((f) => f.c.key),
        encode([
          chosen.map((f) => (f.mode.value === "clear" ? "" : f.input.value)),
        ]),
        true,
        rev,
      );
    },
  );
}
function filterDialog(column = null) {
  dialog(
    "一覧の絞り込み",
    (body) =>
      formFields(
        body,
        column
          ? [
              {
                key: "value",
                label: column.label + "（完全一致）",
                value: S.filter.columns?.[column.key] || "",
              },
            ]
          : [
              { key: "query", label: "全文検索", value: S.filter.query || "" },
              ...["status", "priority", "severity", "review"].map((k) => ({
                key: k,
                label: {
                  status: "状態",
                  priority: "優先度",
                  severity: "重大度",
                  review: "レビュー",
                }[k],
                value: S.filter[k] || "",
                options: [["", "すべて"], ...S.meta.enums[k]],
              })),
              ...["assignee", "category"].map((k) => ({
                key: k,
                label: k === "assignee" ? "担当者" : "カテゴリ",
                value: String(S.filter[k] || ""),
                options: [["", "すべて"], ...enumOptions(k).slice(1)],
              })),
              {
                key: "overdue_before",
                label: "指定日より前の未完了期限",
                value: S.filter.overdue_before || "",
                options: null,
              },
              {
                key: "unassigned",
                label: "担当者未設定のみ",
                value: String(S.filter.unassigned || false),
                options: [
                  ["false", "指定しない"],
                  ["true", "未設定のみ"],
                ],
              },
              {
                key: "incomplete",
                label: "未完了のみ",
                value: String(S.filter.incomplete || false),
                options: [
                  ["false", "指定しない"],
                  ["true", "未完了のみ"],
                ],
              },
            ],
      ),
    async (f) => {
      const v = values(f);
      if (column) {
        S.filter.columns ||= {};
        S.filter.columns[column.key] = v.value;
      } else {
        for (const [k, value] of Object.entries(v)) {
          delete S.filter[k];
          if (value !== "") {
            if (["assignee", "category"].includes(k))
              S.filter[k] = Number(value);
            else if (["unassigned", "incomplete"].includes(k))
              S.filter[k] = value === "true";
            else S.filter[k] = value;
          }
        }
      }
      await reload();
    },
    "適用",
  );
}
function download(text, name, mime = "text/csv;charset=utf-8") {
  const a = el("a", { download: name });
  const url = URL.createObjectURL(new Blob([text], { type: mime }));
  a.href = url;
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
async function copySelected() {
  const text = selectionText();
  if (navigator.clipboard && isSecureContext) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      /* Use the keyboard-compatible path. */
    }
  }
  $("#surface").focus();
  if (document.execCommand("copy")) return;
  dialog(
    "選択セルをコピー",
    (b) => {
      const input = el("textarea", {
        "aria-label": "コピーする内容",
        readonly: true,
      });
      input.value = text;
      b.append(
        el("p", { text: "内容を選択してCtrl+Cでコピーできます。" }),
        input,
      );
      queueMicrotask(() => {
        input.focus();
        input.select();
      });
      return input;
    },
    null,
  );
}
async function backupDatabase() {
  const r = await fetch("/web/backup", {
    method: "POST",
    credentials: "same-origin",
    headers: { "Content-Type": "application/json", "X-YY-CSRF": S.csrf },
    body: "{}",
  });
  if (!r.ok) throw new Error((await r.json()).error);
  download(
    await r.blob(),
    "yyproject-backup.sqlite3",
    "application/vnd.sqlite3",
  );
}
function showHelp() {
  dialog(
    "Web UIの使い方",
    (body) => {
      for (const text of [
        "一覧：セルを選択しF2またはEnterで編集。Shift+Enterで改行、Enterで確定、Escで取り消します。",
        "Ctrl+C/Vは選択セルの範囲を引用符付きTSVでコピー・貼り付けします。ブラウザーの権限制限がある場合も、キーボード操作または貼り付けダイアログが使えます。",
        "同じ属性列の2行以上を選んでCtrl+Dで先頭の値を下にコピー。件名や本文は個別編集です。行チェックボックスとCtrl／Shift選択から属性を一括変更できます。",
        "IDセルをドラッグして行を移動。列境界で列幅、IDセル下の行境界で行高を変更します。列見出しの右クリックで列値を絞り込みます。",
        "カンバンはカードを状態列へドラッグ。ガントの項目・予定日程の境界をドラッグして幅を変更します。表示・統計設定から起点日・稲妻線・集計期間を変更できます。",
        "子タスクは一覧の行を右クリック。依頼・質問のやりとりは「やりとり記録」から手入力します。送受信・添付はありません。",
        "CSV出力で列と条件を選択します。SQLiteへ書き出しは監査を含むサーバーの全案件のバックアップです。",
        "MCPと同じデータを使用し、他の操作による更新は編集中以外に定期反映します。競合時は入力を確認し、再読込してから再編集してください。",
      ])
        body.append(el("p", { text }));
      return null;
    },
    null,
  );
}
async function pasteClipboard() {
  if (navigator.clipboard && isSecureContext) {
    try {
      await pasteText(await navigator.clipboard.readText());
      return;
    } catch (error) {
      if (error.name !== "NotAllowedError") throw error;
    }
  }
  dialog(
    "セルに貼り付け",
    (b) =>
      formFields(b, [
        {
          key: "text",
          label: "ここにCtrl+Vで貼り付けて適用してください",
          large: true,
        },
      ]),
    async (f) => pasteText(f.text.value),
    "適用",
  );
}
function exportMasters() {
  const rows = [...S.rows],
    cols = [...S.cols];
  dialog(
    "マスタCSV・出力列の選択",
    (body) => {
      const checks = cols.map((c) => {
        const input = el("input", { type: "checkbox", checked: true });
        body.append(el("label", {}, [input, c.label]));
        return { c, input };
      });
      return checks;
    },
    async (checks) => {
      const selected = checks.filter((c) => c.input.checked).map((c) => c.c);
      if (!selected.length) throw new Error("出力列を選択してください");
      download(
        "\ufeff" +
          encode(
            [
              selected.map((c) => c.label),
              ...rows.map((r) => selected.map((c) => r.values[c.key] || "")),
            ],
            ",",
          ),
        "yyproject-masters.csv",
      );
    },
    "CSVを保存",
  );
}
function exportDialog(jira = false) {
  if (S.view === "masters") return exportMasters();
  if (!S.p) throw new Error("プロジェクトを選択してください");
  const p = S.p,
    filter = { ...structuredClone(S.filter), kind: kind() },
    cols = jira
      ? S.meta.jira_columns.map(([key, label]) => ({ key, label }))
      : S.meta.columns[kind()];
  dialog(
    jira ? "Jira CSV出力" : "CSV出力・列の選択",
    (body) => {
      const fields = formFields(body, [
        {
          key: "query",
          label: "全文検索（列条件も適用）",
          value: filter.query || "",
        },
        {
          key: "kind",
          label: "種類",
          value: kind(),
          options: [
            ["task", "タスク"],
            ["bug", "バグ"],
            ["issue", "課題"],
            ...(!jira
              ? [
                  ["inquiry", "依頼・質問"],
                  ["workaround", "暫定対応"],
                ]
              : []),
          ],
        },
      ]);
      const jiraFields = jira
        ? formFields(body, [
            {
              key: "epic_roots",
              label: "親・子・孫の対応",
              value: "true",
              options: [
                ["true", "Epic / Task / Sub-task"],
                ["false", "Task / Sub-task"],
              ],
            },
            ...["epic_type", "task_type", "subtask_type", "issue_type"].map(
              (key, i) => ({
                key,
                label: [
                  "Epicの課題タイプ",
                  "タスクの課題タイプ",
                  "子タスクの課題タイプ",
                  "バグ・課題の課題タイプ",
                ][i],
                value: ["Epic", "Task", "Sub-task", "Bug"][i],
              }),
            ),
          ])
        : null;
      const checks = el("div", { class: "column-checks" }),
        inputs = [];
      for (const c of cols) {
        const input = el("input", {
          type: "checkbox",
          checked: true,
          "data-key": c.key,
        });
        checks.append(el("label", {}, [input, c.label]));
        inputs.push(input);
      }
      body.append(checks);
      return { fields, inputs, jiraFields };
    },
    async ({ fields, inputs, jiraFields }) => {
      const f = { ...filter, ...values(fields) };
      const keys = inputs.filter((i) => i.checked).map((i) => i.dataset.key);
      const result = jira
        ? await api({
            action: "jira",
            project_id: p.id,
            filter: f,
            keys,
            options: {
              ...values(jiraFields),
              epic_roots: jiraFields.epic_roots.value === "true",
            },
          })
        : await call("csv_export", {
            project_id: p.id,
            columns: keys,
            filter: f,
          });
      download(
        result.content,
        jira ? "jira-import.csv" : `yyproject-${f.kind}.csv`,
      );
    },
    "CSVを保存",
  );
}
async function archive(id = focusedId()) {
  writable();
  if (S.view === "masters") {
    const rev = S.p.revision;
    dialog(
      "マスタの削除",
      (b) =>
        b.append(
          el("p", {
            text: "選択したマスタを削除します。参照中のマスタは削除できません。",
          }),
        ),
      async () => change({ operation: "remove_master", id }, rev),
      "削除",
    );
    return;
  }
  if (!id) throw new Error("対象を選択してください");
  const rev = S.p.revision,
    impact = await call("removal_preview", { project_id: S.p.id, task_id: id });
  dialog(
    "項目を保管",
    (b) =>
      b.append(
        el("p", {
          text: `#${id} ${impact.task.title}\n依存 ${impact.dependencies.length}件・関連 ${impact.related_issue_ids.length}件。データと関係は残り、復元できます。`,
        }),
      ),
    async () => change({ operation: "remove_task", id }, rev),
    "保管",
  );
}
async function restore(id = focusedId()) {
  if (!id) throw new Error("対象を選択してください");
  await change({ operation: "restore_task", id });
}
async function statusChange(id, status) {
  writable();
  const t = structuredClone(task(id));
  const rev = S.p.revision;
  if (t.status === "done" && status !== "done" && status !== "cancelled") {
    return dialog(
      "項目を再開",
      (b) =>
        formFields(b, [
          { key: "progress", label: "再開後の進捗（0〜99%）", value: "0" },
        ]),
      async (f) => {
        t.status = status;
        t.progress = Number(f.progress.value);
        if (t.progress < 0 || t.progress >= 100)
          throw new Error("0〜99%を指定してください");
        await change({ operation: "save_task", task: t }, rev);
      },
    );
  }
  t.status = status;
  await change({ operation: "save_task", task: t }, rev);
}
function exchangeDialog(id = focusedId()) {
  const t = task(id);
  if (t?.kind !== "inquiry")
    throw new Error("依頼・質問の行を選択してください");
  dialog(
    "やりとり記録",
    (body) => {
      body.append(
        el("button", {
          class: "primary",
          text: "記録を追加",
          disabled: S.readonly,
          onclick: () => exchangeEdit(id, null),
        }),
      );
      for (const entry of t.inquiry.exchanges) {
        body.append(
          el("section", { class: "log-entry" }, [
            el("header", {}, [
              el("span", {
                text: `#${entry.id} ${entry.at} · ${entry.direction === "outgoing" ? "こちらから" : entry.direction === "incoming" ? "相手から" : "メモ"} · ${entry.person}`,
              }),
              el("span", { class: "spacer" }),
              el("button", {
                text: "編集",
                disabled: S.readonly,
                onclick: () => exchangeEdit(id, entry),
              }),
              el("button", {
                text: "削除",
                disabled: S.readonly,
                onclick: () => {
                  const rev = S.p.revision;
                  dialog(
                    "記録の削除",
                    (b) =>
                      b.append(
                        el("p", {
                          text: "このやりとり記録を削除します。監査は保持します。",
                        }),
                      ),
                    async () => {
                      await change(
                        {
                          operation: "remove_exchange",
                          id,
                          entry_id: entry.id,
                        },
                        rev,
                      );
                      document
                        .querySelectorAll("dialog")
                        .forEach((d) => d.close());
                      exchangeDialog(id);
                    },
                    "削除",
                  );
                },
              }),
            ]),
            el("p", { text: entry.text }),
          ]),
        );
      }
      return null;
    },
    null,
  );
}
function exchangeEdit(id, entry) {
  writable();
  const rev = S.p.revision;
  dialog(
    entry ? "やりとりを編集" : "やりとりを記録",
    (b) =>
      formFields(b, [
        {
          key: "at",
          label: "記録日時（タイムゾーン付き）",
          value: entry?.at || new Date().toISOString(),
        },
        {
          key: "direction",
          label: "方向",
          value: entry?.direction || "outgoing",
          options: [
            ["outgoing", "こちらから"],
            ["incoming", "相手から"],
            ["note", "メモ"],
          ],
        },
        { key: "person", label: "相手・記録者", value: entry?.person || "" },
        {
          key: "text",
          label: "やりとりの内容",
          value: entry?.text || "",
          large: true,
        },
      ]),
    async (f) => {
      await change(
        {
          operation: "save_exchange",
          id,
          entry: { ...values(f), id: entry?.id || 0 },
        },
        rev,
      );
      document.querySelectorAll("dialog").forEach((d) => d.close());
      exchangeDialog(id);
    },
  );
}
function dependencyDialog() {
  writable();
  const opts = S.p.tasks
      .filter((t) => !t.archived)
      .map((t) => [t.id, `#${t.id} ${t.title}`]),
    rev = S.p.revision;
  dialog(
    "依存関係",
    (b) => {
      const f = formFields(b, [
        { key: "predecessor", label: "先行項目", options: opts },
        { key: "successor", label: "後続項目", options: opts },
        { key: "lag_days", label: "待機営業日", value: "0" },
      ]);
      for (const d of S.p.dependencies)
        b.append(
          el("p", {}, [
            `#${d.predecessor} → #${d.successor}（${d.lag_days}日） `,
            el("button", {
              text: "解除",
              onclick: guarded(async () => {
                await change(
                  {
                    operation: "remove_dependency",
                    predecessor: d.predecessor,
                    successor: d.successor,
                  },
                  rev,
                );
                document.querySelectorAll("dialog").forEach((d) => d.close());
              }),
            }),
          ]),
        );
      return f;
    },
    async (f) => {
      const v = values(f);
      await change(
        {
          operation: "add_dependency",
          dependency: {
            predecessor: Number(v.predecessor),
            successor: Number(v.successor),
            lag_days: Number(v.lag_days),
          },
        },
        rev,
      );
    },
    "依存関係を追加",
  );
}
async function scheduleDialog() {
  writable();
  const p = S.p,
    rev = p.revision,
    preview = await call("schedule_preview", { project_id: p.id });
  dialog(
    "日程再計算のプレビュー",
    (body) => {
      for (const c of preview.changes)
        body.append(
          el("p", {
            text: `#${c.id} ${c.old_start || "未定"}～${c.old_due || "未定"} → ${c.start}～${c.due}`,
          }),
        );
      body.append(
        el("p", {
          text: `完了予定日を超える項目：${preview.overdue_ids.join(", ") || "なし"}`,
        }),
      );
      return formFields(body, [
        {
          key: "allow",
          label: "予定日超過を許可して適用",
          value: "false",
          options: [
            ["false", "許可しない"],
            ["true", "明示的に許可する"],
          ],
        },
      ]);
    },
    async (f) =>
      change(
        { operation: "schedule", allow_overrun: f.allow.value === "true" },
        rev,
      ),
    "適用",
  );
}
function today() {
  return new Date().toISOString().slice(0, 10);
}
function addDay(d, n) {
  return new Date(Date.parse(d + "T00:00:00Z") + n * DAY)
    .toISOString()
    .slice(0, 10);
}
function days(a, b) {
  return Math.round(
    (Date.parse(b + "T00:00:00Z") - Date.parse(a + "T00:00:00Z")) / DAY,
  );
}
function workday(date) {
  const day = new Date(date + "T00:00:00Z").getUTCDay();
  return (
    day !== 0 &&
    day !== 6 &&
    !S.p.masters.some((m) => m.kind === "holiday" && m.date === date)
  );
}
function color(status) {
  return (
    {
      todo: "#7891b6",
      doing: "#4385c7",
      blocked: "#cf9653",
      done: "#4f9a78",
      cancelled: "#9ca3af",
    }[status] || "#7891b6"
  );
}
function renderBoard() {
  const board = el("div", { class: "board" });
  for (const [status, label] of S.meta.enums.status) {
    const records = S.rows.filter((r) => task(r.id).status === status);
    const lane = el(
      "section",
      { class: "lane", "data-status": status },
      el("h2", {}, [
        label,
        el("span", { class: "pill", text: records.length }),
      ]),
    );
    for (const r of records) {
      const t = task(r.id),
        card = el(
          "article",
          {
            class: "card",
            draggable: !S.readonly && !t.archived,
            "data-id": t.id,
            tabindex: 0,
            onclick: () => {
              S.focus = t.id;
              document
                .querySelectorAll(".card")
                .forEach((c) =>
                  c.classList.toggle("focused", Number(c.dataset.id) === t.id),
                );
            },
            oncontextmenu: (e) => {
              e.preventDefault();
              S.focus = t.id;
              menu(e, [
                ["項目を編集", () => editItem(t.id)],
                ["子タスクを追加", () => editItem(null, t.id)],
                ...S.meta.enums.status.map(([s, label]) => [
                  `${label}に変更`,
                  () => statusChange(t.id, s),
                ]),
              ]);
            },
            ondblclick: guarded(() => editItem(t.id)),
            onkeydown: guarded((e) => {
              if (e.key === "Enter") return editItem(t.id);
            }),
          },
          [
            el("div", { class: "meta" }, [
              el("span", { text: "#" + t.id }),
              el("span", {
                class: "pill",
                text: r.values.category || "未分類",
              }),
            ]),
            el("h3", { text: t.title }),
            el("div", { class: "meta" }, [
              el("span", { text: r.values.assignee || "未割当" }),
              el("span", { text: t.due || "期限未定" }),
            ]),
          ],
        );
      card.style.borderLeftColor = color(status);
      const fill = el("div");
      fill.style.width = t.progress + "%";
      card.append(el("div", { class: "progress" }, fill));
      card.addEventListener("dragstart", (e) => {
        S.drag = { type: "card", id: t.id, revision: S.p.revision };
        e.dataTransfer.setData("text/plain", String(t.id));
        e.dataTransfer.effectAllowed = "move";
        card.classList.add("dragging");
      });
      card.addEventListener("dragend", () => {
        S.drag = null;
        card.classList.remove("dragging");
        clearDrop();
      });
      lane.append(card);
    }
    lane.addEventListener("dragover", (e) => {
      if (S.drag?.type === "card") {
        e.preventDefault();
        clearDrop();
        lane.classList.add("drag-over");
        edgeScroll(e);
      }
    });
    lane.addEventListener(
      "drop",
      guarded(async (e) => {
        e.preventDefault();
        const drag = S.drag;
        S.drag = null;
        clearDrop();
        if (drag) {
          if (drag.revision !== S.p.revision)
            throw new Error("再読込してから移動してください");
          await statusChange(drag.id, status);
        }
      }),
    );
    board.append(lane);
  }
  $("#surface").append(board);
}
function settingsDialog() {
  const gantt = S.view === "gantt",
    range = statRange();
  dialog(
    gantt ? "ガント表示設定" : "統計の集計期間",
    (b) =>
      formFields(
        b,
        gantt
          ? [
              {
                key: "origin",
                label: "表示起点",
                value: S.layout.origin || S.p.start,
              },
              {
                key: "daywidth",
                label: "1日の幅（px）",
                value: S.layout.daywidth || 26,
              },
              {
                key: "namewidth",
                label: "項目・予定日程の幅（px）",
                value: S.layout.namewidth || 360,
              },
              {
                key: "lightning",
                label: "稲妻線",
                value: String(S.layout.lightning !== false),
                options: [
                  ["true", "表示"],
                  ["false", "非表示"],
                ],
              },
              {
                key: "reference",
                label: "稲妻線の基準日",
                value: S.layout.reference || today(),
              },
            ]
          : [
              { key: "from", label: "開始日", value: range[0] },
              { key: "to", label: "終了日", value: range[1] },
            ],
      ),
    async (f) => {
      const v = values(f);
      if (gantt) {
        if (
          !/^\d{4}-\d{2}-\d{2}$/.test(v.origin) ||
          !/^\d{4}-\d{2}-\d{2}$/.test(v.reference)
        )
          throw new Error("日付はYYYY-MM-DDで入力してください");
        S.layout.origin = v.origin;
        S.layout.reference = v.reference;
        S.layout.daywidth = Math.max(
          12,
          Math.min(100, Number(v.daywidth) || 26),
        );
        S.layout.namewidth = Math.max(
          180,
          Math.min(1200, Number(v.namewidth) || 360),
        );
        S.layout.lightning = v.lightning === "true";
        persistLayout();
      } else {
        if (
          days(v.from, v.to) < 0 ||
          days(v.from, v.to) > 3660 ||
          !v.from ||
          !v.to
        )
          throw new Error("集計期間は10年以内で指定してください");
        S.range = [v.from, v.to];
      }
      await render();
    },
    "適用",
  );
}
function calendar(svgRoot, origin, count, step, left, y, reverse = false) {
  const labels = ["年", "月", "日", "曜"];
  for (let i = 0; i < count; i++) {
    const d = addDay(origin, i),
      dt = new Date(d + "T00:00:00Z"),
      x = left + i * step,
      weekday = ["日", "月", "火", "水", "木", "金", "土"][dt.getUTCDay()];
    for (let row = 0; row < 4; row++) {
      const level = reverse ? 3 - row : row;
      let text = "";
      if (level === 0 && (i === 0 || d.slice(5) === "01-01"))
        text = dt.getUTCFullYear() + "年";
      if (level === 1 && (i === 0 || d.endsWith("-01")))
        text = dt.getUTCMonth() + 1 + "月";
      if (level === 2 && (!reverse || i % 7 === 0))
        text = String(dt.getUTCDate());
      if (level === 3) text = weekday;
      svgRoot.append(
        svg("line", {
          x1: x,
          y1: y + row * 23,
          x2: x,
          y2: y + (row + 1) * 23,
          stroke: "#e0e6ef",
        }),
      );
      if (text)
        svgRoot.append(
          svg(
            "text",
            {
              x: x + 3,
              y: y + row * 23 + 16,
              class: "svg-text",
              fill: workday(d) ? "#4f617c" : "#bc5555",
              "aria-label": labels[level] + text,
            },
            text,
          ),
        );
    }
  }
}
function renderGantt() {
  const rows = S.rows.filter((r) => task(r.id).kind === "task");
  const origin = S.layout.origin || S.p.start,
    nameW = S.layout.namewidth || 360,
    step = S.layout.daywidth || 26,
    count = Math.min(3661, Math.max(60, days(origin, S.p.finish) + 1));
  let tops = [96];
  for (const r of rows) tops.push(tops.at(-1) + height(r.id));
  const total = tops.at(-1),
    w = nameW + count * step,
    wrap = el("div", { class: "timeline" });
  wrap.style.width = w + "px";
  wrap.style.height = total + "px";
  const drawing = svg("svg", {
    width: w,
    height: total,
    role: "img",
    "aria-label": "ガント・年・月・日・曜日",
  });
  drawing.style.position = "absolute";
  drawing.style.top = "0";
  for (let i = 0; i < count; i++) {
    const date = addDay(origin, i),
      x = nameW + i * step;
    if (!workday(date))
      drawing.append(
        svg("rect", { x, y: 0, width: step, height: total, fill: "#f0f1f4" }),
      );
    drawing.append(
      svg("line", { x1: x, y1: 96, x2: x, y2: total, stroke: "#e2e7ef" }),
    );
  }
  calendar(drawing, origin, count, step, nameW, 0);
  let lightning = `${nameW + days(origin, S.layout.reference || today()) * step},96`;
  rows.forEach((r, index) => {
    const t = task(r.id),
      y = tops[index],
      h = height(r.id);
    drawing.append(
      svg("line", {
        x1: nameW,
        y1: y + h,
        x2: w,
        y2: y + h,
        stroke: "#e2e7ef",
      }),
    );
    if (t.start && t.due) {
      const x = nameW + days(origin, t.start) * step,
        bw = Math.max(9, (days(t.start, t.due) + 1) * step);
      let bar, progress;
      if (t.milestone) {
        bar = svg("polygon", {
          points: `${x},${y + h / 2 - 8} ${x + 8},${y + h / 2} ${x},${y + h / 2 + 8} ${x - 8},${y + h / 2}`,
          fill: "#3b6db7",
        });
      } else {
        bar = svg("rect", {
          x,
          y: y + 9,
          width: bw,
          height: Math.max(10, h - 18),
          rx: 3,
          fill: color(t.status),
          class: "gantt-bar",
          "data-id": t.id,
        });
        progress = svg("rect", {
          x,
          y: y + 9,
          width: (bw * t.progress) / 100,
          height: Math.max(10, h - 18),
          rx: 3,
          fill: "#224977",
          opacity: 0.4,
          "pointer-events": "none",
        });
        if (!S.readonly && !t.archived)
          bar.addEventListener("pointerdown", (e) => {
            e.preventDefault();
            S.focus = t.id;
            const start = e.clientX,
              rev = S.p.revision;
            let delta = 0;
            bar.setPointerCapture(e.pointerId);
            const move = (e) => {
              delta = Math.round((e.clientX - start) / step);
              bar.setAttribute("x", x + delta * step);
              progress.setAttribute("x", x + delta * step);
            };
            let active = true;
            const cleanup = () => {
              active = false;
              bar.removeEventListener("pointermove", move);
              bar.removeEventListener("pointerup", end);
              bar.removeEventListener("pointercancel", cancel);
              bar.removeEventListener("lostpointercapture", cancel);
              document.removeEventListener("keydown", key);
              if (bar.hasPointerCapture(e.pointerId))
                bar.releasePointerCapture(e.pointerId);
            };
            const cancel = () => {
              if (!active) return;
              delta = 0;
              bar.setAttribute("x", x);
              progress.setAttribute("x", x);
              cleanup();
            };
            const key = (e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                cancel();
              }
            };
            const end = guarded(async (e) => {
              if (!active) return;
              cleanup();
              if (delta) {
                const moved = structuredClone(t);
                moved.start = addDay(t.start, delta);
                moved.due = addDay(t.due, delta);
                await change({ operation: "save_task", task: moved }, rev);
              }
            });
            bar.addEventListener("pointermove", move);
            bar.addEventListener("pointerup", end);
            bar.addEventListener("pointercancel", cancel);
            bar.addEventListener("lostpointercapture", cancel);
            document.addEventListener("keydown", key);
          });
      }
      bar.append(
        svg(
          "title",
          {},
          `#${t.id} ${t.title} ${t.start}～${t.due} ${t.progress}%`,
        ),
      );
      drawing.append(bar);
      if (progress) drawing.append(progress);
      bar.addEventListener("click", () => {
        S.focus = t.id;
      });
      bar.addEventListener(
        "dblclick",
        guarded(() => editItem(t.id)),
      );
      const ref = S.layout.reference || today();
      const point =
        t.status === "done" || t.start > ref
          ? nameW + days(origin, ref) * step
          : x + (bw * t.progress) / 100;
      lightning += ` ${point},${y + h / 2}`;
    }
  });
  if (S.layout.lightning !== false)
    drawing.append(
      svg("polyline", {
        points: lightning,
        fill: "none",
        stroke: "#d47629",
        "stroke-width": 4,
        "pointer-events": "none",
        "aria-label": "稲妻線",
      }),
    );
  wrap.append(drawing);
  const names = el("div", { class: "timeline-name" });
  names.style.width = nameW + "px";
  names.style.height = total + "px";
  const title = el("div", { class: "gantt-row", text: "項目・予定日程" });
  title.style.top = "0";
  title.style.width = nameW + "px";
  title.style.height = "96px";
  names.append(title);
  rows.forEach((r, i) => {
    const t = task(r.id),
      h = height(t.id),
      name = el(
        "div",
        {
          class: "gantt-row",
          "data-task-id": t.id,
          title: t.title,
          onclick: () => {
            S.focus = t.id;
            document
              .querySelectorAll(".gantt-row")
              .forEach((row) => row.classList.remove("focused"));
            name.classList.add("focused");
          },
          ondblclick: guarded(() => editItem(t.id)),
        },
        [
          outlineTitle(r),
          el("small", { text: `${t.start || "未定"} ～ ${t.due || "未定"}` }),
        ],
      );
    name.style.top = tops[i] + "px";
    name.style.height = h + "px";
    name.style.width = nameW + "px";
    name.append(
      el("div", {
        class: "row-resize",
        onpointerdown: (e) =>
          resize(
            e,
            (_, dy) => {
              S.layout[`row:${S.p.id}:${S.view}:${t.id}`] = Math.max(
                32,
                Math.min(500, h + dy),
              );
              name.style.height = height(t.id) + "px";
            },
            () => {
              persistLayout();
              $("#surface").replaceChildren();
              renderGantt();
            },
          ),
      }),
    );
    names.append(name);
  });
  const divider = el("div", {
    class: "gantt-divider",
    "aria-label": "項目・予定日程の列幅",
    onpointerdown: (e) =>
      resize(
        e,
        (dx) => {
          S.layout.namewidth = Math.max(180, Math.min(1200, nameW + dx));
          names.style.width = S.layout.namewidth + "px";
        },
        () => {
          $("#surface").replaceChildren();
          renderGantt();
        },
      ),
  });
  divider.style.right = "-3px";
  names.append(divider);
  wrap.append(names);
  $("#surface").append(wrap);
}
function renderNetwork() {
  const rows = S.data.rows.filter((r) => !task(r.id).archived),
    ids = new Set(rows.map((r) => r.id)),
    edges = S.p.dependencies.filter(
      (e) => ids.has(e.predecessor) && ids.has(e.successor),
    );
  const related = new Set(S.focus ? [S.focus] : []);
  if (S.focus) {
    for (const reverse of [false, true]) {
      const queue = [S.focus],
        seen = new Set(queue);
      while (queue.length) {
        const id = queue.shift();
        for (const edge of edges) {
          const from = reverse ? edge.successor : edge.predecessor,
            to = reverse ? edge.predecessor : edge.successor;
          if (from === id && !seen.has(to)) {
            seen.add(to);
            queue.push(to);
            related.add(to);
          }
        }
      }
    }
  }
  const coords = S.data.network,
    w = Math.max(900, ...Object.values(coords).map((v) => v[0] * 260 + 270)),
    h = Math.max(450, ...Object.values(coords).map((v) => v[1] * 85 + 110)),
    drawing = svg("svg", {
      width: w,
      height: h,
      role: "img",
      "aria-label": "依存ネットワーク",
    });
  const defs = svg("defs"),
    marker = svg("marker", {
      id: "arrow",
      viewBox: "0 0 10 10",
      refX: 9,
      refY: 5,
      markerWidth: 7,
      markerHeight: 7,
      orient: "auto-start-reverse",
    });
  marker.append(svg("path", { d: "M 0 0 L 10 5 L 0 10 z", fill: "#7891b1" }));
  defs.append(marker);
  drawing.append(defs);
  for (const edge of edges) {
    const a = coords[edge.predecessor],
      b = coords[edge.successor];
    if (!a || !b) continue;
    drawing.append(
      svg("path", {
        d: `M ${a[0] * 260 + 230} ${a[1] * 85 + 48} C ${a[0] * 260 + 265} ${a[1] * 85 + 48} ${b[0] * 260 - 15} ${b[1] * 85 + 48} ${b[0] * 260 + 20} ${b[1] * 85 + 48}`,
        fill: "none",
        stroke: "#7b92b2",
        "stroke-width": 2,
        "marker-end": "url(#arrow)",
        opacity:
          S.focus &&
          !(related.has(edge.predecessor) && related.has(edge.successor))
            ? 0.12
            : 1,
      }),
    );
  }
  for (const r of rows) {
    const t = task(r.id),
      v = coords[t.id];
    if (!v) continue;
    const g = svg("g", {
      transform: `translate(${v[0] * 260 + 20},${v[1] * 85 + 20})`,
      class:
        "network-node" +
        (S.focus && !related.has(t.id) ? " network-muted" : ""),
      tabindex: 0,
      role: "button",
      "aria-label": t.title,
    });
    g.append(
      svg("rect", {
        width: 210,
        height: 57,
        rx: 5,
        fill: S.focus === t.id ? "#dfebff" : "#f4f7fc",
        stroke: color(t.status),
        "stroke-width": S.focus === t.id ? 3 : 1,
      }),
      svg(
        "text",
        { x: 10, y: 22, class: "svg-text" },
        `#${t.id} ${t.title.slice(0, 22)}`,
      ),
      svg("text", { x: 10, y: 43, class: "svg-text" }, r.values.status),
      svg("title", {}, t.title),
    );
    const select = () => {
      S.focus = S.focus === t.id ? null : t.id;
      $("#surface").replaceChildren();
      renderNetwork();
    };
    g.addEventListener("click", select);
    g.addEventListener("keydown", (e) => {
      if (e.key === "Enter") select();
    });
    g.addEventListener(
      "dblclick",
      guarded(() => editItem(t.id)),
    );
    drawing.append(g);
  }
  $("#surface").append(drawing);
}
function statRange() {
  if (S.range) return S.range;
  if (isStats()) {
    const dates = S.p.tasks
      .filter((t) => t.kind === kind())
      .map((t) =>
        (
          t.case?.detected_at ||
          t.inquiry?.requested_at ||
          t.workaround?.detected_at ||
          today()
        ).slice(0, 10),
      )
      .sort();
    return [dates[0] || S.p.start, today()];
  }
  const history = S.p.history;
  return [
    history[0]?.day < S.p.start ? history[0].day : S.p.start,
    history.at(-1)?.day || S.p.finish,
  ];
}
function metric(n, label) {
  return el("div", { class: "metric" }, [
    el("strong", { text: n ?? "未算出" }),
    el("span", { text: label }),
  ]);
}
async function renderStatistics() {
  const wrap = el("div", { class: "chart-wrap" });
  $("#surface").append(wrap);
  const [from, to] = statRange();
  let history = S.p.history,
    stats = null;
  const management = ["inquiry", "workaround"].includes(kind());
  if (isStats()) {
    const current = S.view;
    stats = await call(
      management ? "management_statistics" : "case_statistics",
      { project_id: S.p.id, kind: kind(), from, to, filter: S.filter },
    );
    if (S.view !== current) return;
    history = stats.daily.map((d) => ({
      day: d.day,
      remaining: d.open,
      completed: d.resolved,
      scope: d.detected,
    }));
    const summary = el("div", { class: "chart-summary" }, [
      metric(stats.total, "対象件数"),
      metric(stats.open, "未完了"),
      metric(stats.resolved, "完了"),
      metric(stats.cancelled, "中止"),
    ]);
    if (management) {
      summary.append(
        metric(stats.overdue, "期限超過"),
        metric(
          management && kind() === "inquiry"
            ? stats.unanswered
            : stats.not_applied,
          kind() === "inquiry" ? "未回答" : "暫定未実施",
        ),
      );
      const mean =
        kind() === "inquiry"
          ? stats.mean_answer_hours
          : stats.mean_temporary_hours;
      summary.append(
        metric(
          mean == null ? null : mean.toFixed(1) + "h",
          kind() === "inquiry" ? "平均回答時間" : "平均暫定対応時間",
        ),
      );
      if (kind() === "workaround")
        summary.append(
          metric(
            stats.mean_resolution_hours == null
              ? null
              : stats.mean_resolution_hours.toFixed(1) + "h",
            "平均恒久対応時間",
          ),
        );
      if (kind() === "inquiry")
        summary.append(
          metric(
            `${stats.outgoing} / ${stats.incoming} / ${stats.notes}`,
            "期間内 こちら／相手／メモ",
          ),
        );
    }
    wrap.append(summary);
  }
  wrap.append(
    el("p", {
      class: "chart-caption",
      text: `${from} ～ ${to}（UTC日付） · ${S.view === "up" ? "完了工数（青）・総工数（灰）" : isStats() ? "未完了件数（青）。完了と中止を除き、再オープンを反映。" : "残工数（青）・理想線（点線）"} · 日次格子・休日背景・週ごとの日付表示`,
    }),
  );
  const count = Math.max(1, Math.min(3661, days(from, to) + 1)),
    step = 24,
    w = Math.max(950, count * step + 100),
    plotW = w - 100,
    h = 310,
    left = 65,
    top = 15;
  const drawing = svg("svg", {
    width: w,
    height: h + 135,
    role: "img",
    "aria-label": view()[1] + "の日次チャート",
  });
  let last = { scope: 0, remaining: 0, completed: 0 };
  const points = [];
  for (let i = 0; i < count; i++) {
    const date = addDay(from, i);
    const available = history.filter((s) => s.day <= date);
    last = available.at(-1) || last;
    points.push({ ...last, day: date });
  }
  const up = S.view === "up",
    rawMax = Math.max(
      1,
      ...points.map((p) => (up ? Math.max(p.scope, p.completed) : p.remaining)),
    ),
    unit = isStats()
      ? 1
      : Math.max(0.1, 10 ** Math.floor(Math.log10(rawMax / 5))),
    tick = Math.max(unit, Math.ceil(rawMax / 5 / unit) * unit),
    max = tick * 5,
    dx = plotW / count;
  for (let i = 0; i < count; i++) {
    const x = left + i * dx;
    if (!workday(points[i].day))
      drawing.append(
        svg("rect", { x, y: top, width: dx, height: h + 46, fill: "#f0f1f4" }),
      );
    drawing.append(
      svg("line", { x1: x, y1: top, x2: x, y2: top + h, stroke: "#e4e8ef" }),
    );
  }
  for (let i = 0; i <= 5; i++) {
    const y = top + h - (i * h) / 5;
    drawing.append(
      svg("line", { x1: left, y1: y, x2: w - 25, y2: y, stroke: "#dce3ed" }),
      svg(
        "text",
        { x: 2, y: y + 4, class: "svg-text" },
        ((max * i) / 5).toFixed(isStats() ? 0 : 1) + (isStats() ? "件" : "h"),
      ),
    );
  }
  const path = (key) =>
    points
      .map((p, i) => `${left + (i + 0.5) * dx},${top + h - (p[key] / max) * h}`)
      .join(" ");
  if (up)
    drawing.append(
      svg("polyline", {
        points: path("scope"),
        fill: "none",
        stroke: "#a4afbd",
        "stroke-width": 3,
      }),
    );
  drawing.append(
    svg("polyline", {
      points: path(up ? "completed" : "remaining"),
      fill: "none",
      stroke: "#2d6cc6",
      "stroke-width": 4,
    }),
  );
  if (!up && !isStats() && points.length > 1)
    drawing.append(
      svg("line", {
        x1: left + dx / 2,
        y1: top + h - (points[0].remaining / max) * h,
        x2: w - 40,
        y2: top + h,
        stroke: "#7e9fca",
        "stroke-width": 2,
        "stroke-dasharray": "6 5",
      }),
    );
  calendar(drawing, from, count, dx, left, top + h, true);
  wrap.append(drawing);
  if (stats) {
    const panels = management
      ? kind() === "inquiry"
        ? [
            ["相手チーム", stats.causes],
            ["依頼／質問", stats.classifications],
            ["状態", stats.states],
          ]
        : [
            ["暫定対応の分類", stats.classifications],
            ["状態", stats.states],
          ]
      : [
          ["原因分類", stats.causes],
          ["発生工程", stats.occurrence_phases],
          ["検出工程", stats.detection_phases],
          ["暫定対策難易度", stats.temporary_difficulties],
          ["恒久対策難易度", stats.permanent_difficulties],
          ["検知難易度", stats.detection_difficulties],
        ];
    wrap.append(
      el("p", {
        class: "chart-caption",
        text: "分類と絞り込みは現在の属性、件数は集計終了日末です。記録のない日時は推測しません。",
      }),
    );
    const analyses = el("div", { class: "analyses" });
    for (const [label, map] of panels) {
      const panel = el(
        "section",
        { class: "analysis" },
        el("h3", { text: label }),
      );
      const entries = Object.entries(map).sort((a, b) => b[1] - a[1]);
      if (!entries.length)
        panel.append(el("p", { class: "muted", text: "対象データなし" }));
      for (const [name, n] of entries) {
        const bar = el("div"),
          percent = ((n / Math.max(stats.total, 1)) * 100).toFixed(1);
        bar.style.width = percent + "%";
        panel.append(
          el("div", { class: "bar-row" }, [
            el("span", { text: name || "未分類" }),
            el("div", { class: "bar" }, bar),
            el("span", { text: `${n}件 (${percent}%)` }),
          ]),
        );
      }
      analyses.append(panel);
    }
    wrap.append(analyses);
  }
}
function wire() {
  for (const [name, label] of VIEWS)
    $("#views").append(
      el("button", {
        "data-view": name,
        text: label,
        "aria-current": String(name === S.view),
        onclick: guarded(() => switchView(name)),
      }),
    );
  $("#login-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    try {
      await authenticated(
        await request("/web/login", { password: $("#password").value }),
      );
    } catch (err) {
      $("#login-error").textContent = err.message;
    }
  });
  $("#logout").onclick = guarded(async () => {
    await request("/web/logout", {});
    showLogin();
  });
  $("#projects").onchange = guarded(() => {
    S.layout.project = Number($("#projects").value);
    persistLayout();
    S.range = null;
    S.filter = {};
    return reload();
  });
  const handlers = {
    backup: backupDatabase,
    help: showHelp,
    "new-project": () => projectDialog(false),
    "rename-project": () => projectDialog(true),
    refresh: () => reload(),
    add: () => editItem(null),
    edit: () => {
      const id = focusedId();
      if (!id) throw new Error("項目を選択してください");
      return editItem(id);
    },
    bulk: bulkDialog,
    fill: fillDown,
    copy: copySelected,
    paste: pasteClipboard,
    filters: () => filterDialog(),
    "clear-filter": async () => {
      S.filter = {};
      await reload();
    },
    export: () => exportDialog(false),
    jira: () => exportDialog(true),
    archive: () => archive(),
    restore: () => restore(),
    exchange: () => exchangeDialog(),
    dependency: dependencyDialog,
    schedule: scheduleDialog,
    capture: () => change({ operation: "capture", day: today() }),
    settings: settingsDialog,
  };
  for (const [id, f] of Object.entries(handlers))
    $("#" + id).onclick = guarded(f);
  $("#search").addEventListener(
    "keydown",
    guarded(async (e) => {
      if (e.key === "Enter") {
        S.filter.query = e.target.value;
        await reload();
      }
    }),
  );
  $("#archived").onchange = guarded(async (e) => {
    S.filter.archived = e.target.checked;
    await reload();
  });
  document.addEventListener("pointerup", () => (S.selecting = false));
  document.addEventListener("click", (e) => {
    if (!e.target.closest("#context-menu")) $("#context-menu").hidden = true;
  });
  const surface = $("#surface");
  surface.addEventListener("copy", (e) => {
    if (!isGrid() || S.editor) return;
    e.preventDefault();
    e.clipboardData.setData("text/plain", selectionText());
  });
  surface.addEventListener(
    "paste",
    guarded(async (e) => {
      if (!isGrid() || S.editor) return;
      e.preventDefault();
      await pasteText(e.clipboardData.getData("text/plain"));
    }),
  );
  surface.addEventListener(
    "keydown",
    guarded(async (e) => {
      if (
        !S.editor &&
        e.ctrlKey &&
        e.code === "Space" &&
        ["tasks", "gantt"].includes(S.view)
      ) {
        e.preventDefault();
        const id = S.view === "gantt" ? S.focus : focusedId();
        if (id != null && childCount(id)) await toggleOutline(id);
        return;
      }
      if (S.editor || !isGrid() || !S.rows.length) return;
      const ctrl = e.ctrlKey || e.metaKey;
      if (ctrl && e.key.toLowerCase() === "a") {
        e.preventDefault();
        S.anchor = [0, 0];
        S.current = [S.rows.length - 1, S.cols.length - 1];
        S.selected.clear();
        selectionPaint();
        return;
      }
      if (ctrl && e.key.toLowerCase() === "d") {
        e.preventDefault();
        await fillDown();
        return;
      }
      if (e.key === "F2" || e.key === "Enter") {
        e.preventDefault();
        await editCell(...S.current);
        return;
      }
      if (e.key === "Delete") {
        e.preventDefault();
        await pasteText("");
        return;
      }
      if (e.key === "Escape") {
        S.selected.clear();
        S.anchor = [...S.current];
        selectionPaint();
        return;
      }
      const directions = {
        ArrowDown: [1, 0],
        ArrowUp: [-1, 0],
        ArrowRight: [0, 1],
        ArrowLeft: [0, -1],
        Tab: [0, e.shiftKey ? -1 : 1],
      };
      if (directions[e.key]) {
        e.preventDefault();
        const d = directions[e.key];
        S.current = [
          Math.max(0, Math.min(S.rows.length - 1, S.current[0] + d[0])),
          Math.max(0, Math.min(S.cols.length - 1, S.current[1] + d[1])),
        ];
        if (!e.shiftKey || e.key === "Tab") S.anchor = [...S.current];
        S.selected.clear();
        const top = gridOffsets[S.current[0]];
        if (top < surface.scrollTop) surface.scrollTop = top;
        else if (top + 80 > surface.scrollTop + surface.clientHeight)
          surface.scrollTop = top + 80 - surface.clientHeight;
        drawGridRows();
        surface
          .querySelector(
            `td[data-row="${S.current[0]}"][data-col="${S.current[1]}"]`,
          )
          ?.scrollIntoView({ block: "nearest", inline: "nearest" });
        selectionPaint();
      } else if (!ctrl && !e.altKey && e.key.length === 1) {
        e.preventDefault();
        await editCell(...S.current, e.key);
      }
    }),
  );
  document.addEventListener("keydown", (e) => {
    if (
      (e.ctrlKey || e.metaKey) &&
      e.key.toLowerCase() === "f" &&
      !$("#app").hidden &&
      !document.querySelector("dialog[open]")
    ) {
      e.preventDefault();
      $("#search").focus();
      $("#search").select();
    }
  });
}
wire();
void request("/web/session", null, "GET")
  .then(authenticated)
  .catch(() => showLogin());
setInterval(async () => {
  if (
    !S.csrf ||
    !S.p ||
    S.editor ||
    S.mutating ||
    S.drag ||
    document.querySelector("dialog[open]") ||
    document.activeElement?.matches("input,select,textarea")
  )
    return;
  const id = S.p.id,
    rev = S.p.revision;
  try {
    const listing = await call("project_list");
    if (
      S.p?.id === id &&
      !S.editor &&
      !S.mutating &&
      !document.querySelector("dialog[open]") &&
      listing.projects.some((p) => p.id === id && p.revision !== rev)
    ) {
      await reload(false);
      message("他の操作による更新を読み込みました", true);
    }
  } catch {
    /* A foreground action reports connectivity errors. */
  }
}, 5000);
