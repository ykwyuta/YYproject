/* Real pointer, keyboard, clipboard and download acceptance against an owned server. */
const { chromium } = require("playwright");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const origin = process.argv[2],
  out = path.resolve("target/web-validation");
fs.mkdirSync(out, { recursive: true });
async function main() {
  const browser = await chromium.launch({
    headless: true,
    ...(process.env.YYPROJECT_BROWSER_CHANNEL
      ? { channel: process.env.YYPROJECT_BROWSER_CHANNEL }
      : {}),
  });
  const context = await browser.newContext({
    viewport: { width: 1440, height: 1000 },
    deviceScaleFactor: 1,
    permissions: ["clipboard-read", "clipboard-write"],
  });
  const deadline = setTimeout(() => {
    void browser.close();
  }, 140000);
  const page = await context.newPage();
  page.setDefaultTimeout(10000);
  const errors = [],
    checks = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => {
    if (m.type() === "error" && !m.text().includes("401"))
      errors.push(m.text());
  });
  const waitAPI = (action,predicate=()=>true) =>
    page.waitForResponse(
      (r) =>
        r.url() === origin + "/web/api" &&
        r.request().postDataJSON()?.action === action && predicate(r.request().postDataJSON()),
    );
  const waitSave = () =>
    page.waitForFunction(() =>
      document.querySelector("#message").textContent.includes("保存"),
    );
  const cell = (r, c) => page.locator(`td[data-row="${r}"][data-col="${c}"]`);
  const nav = async (label) => {
    const response = waitAPI("project");
    await page
      .locator("#views")
      .getByRole("button", { name: label, exact: true })
      .click();
    await response;
    await page.getByRole("heading", { name: label, exact: true }).waitFor();
  };
  const project = async (id) => {
    const response = waitAPI("project",r=>r.project_id===id);
    await page.locator("#projects").selectOption(String(id));
    const data = await (await response).json();
    await page.waitForFunction(
      (n) =>
        document.querySelector("#revision").textContent.includes(n + "項目"),
      data.project.tasks.length,
    );
  };
  const col = async (label) =>
    page.locator("th").evaluateAll((nodes, label) => {
      const th = nodes.find((e) =>
        [label, label + "（個別）"].includes(
          e.querySelector(".th-label")?.textContent,
        ),
      );
      if (!th) throw new Error("Column not found: " + label);
      return Number(th.dataset.col);
    }, label);
  const readProject = async (id) =>
    page.evaluate(async (id) => {
      const s = await (await fetch("/web/session")).json();
      return (
        await (
          await fetch("/web/api", {
            method: "POST",
            headers: {
              "Content-Type": "application/json",
              "X-YY-CSRF": s.csrf,
            },
            body: JSON.stringify({ action: "project", project_id: id }),
          })
        ).json()
      ).project;
    }, id);
  const closeDialog = async () => {
    const d = page.getByRole("dialog").last();
    await d.getByRole("button", { name: "閉じる", exact: true }).last().click();
  };
  try {
    await page.goto(origin);
    await page.locator("#password").fill(process.env.YYPROJECT_TEST_PASSWORD);
    await page.getByRole("button", { name: "ログイン", exact: true }).click();
    await page.locator("#app").waitFor({ state: "visible" });
    await page.waitForFunction(
      () => document.querySelector("#projects").options.length === 4,
    );
    await project(4);
    await nav("依頼・質問一覧");
    await page.locator(".grid").waitFor();
    const team = await col("相手チーム");
    await cell(0, team).scrollIntoViewIfNeeded();
    await cell(0, team).click();
    await page.keyboard.press("F2");
    await page.locator(".cell-editor").fill("ブラウザー連携");
    await page.keyboard.press("Enter");
    await waitSave();
    assert.match(await cell(0, team).textContent(), /ブラウザー連携/);
    await cell(0, team).click();
    await page.keyboard.press("Shift+ArrowDown");
    await page.keyboard.press("Control+d");
    await waitSave();
    assert.match(await cell(1, team).textContent(), /ブラウザー連携/);
    await page.keyboard.press("Control+c");
    const copied = await page.evaluate(() => navigator.clipboard.readText());
    assert.equal(
      copied.replaceAll("\r\n", "\n"),
      "ブラウザー連携\nブラウザー連携\n",
    );
    checks.push(
      "F2/Enter inline edit, range selection, Ctrl+D and real system clipboard copy",
    );
    await page.locator("#bulk").click();
    const bulk = page.getByRole("dialog");
    assert.equal(
      await bulk.getByLabel("件名の変更", { exact: true }).count(),
      0,
    );
    await bulk.getByLabel("担当者の変更", { exact: true }).selectOption("set");
    await bulk.getByLabel("担当者", { exact: true }).selectOption("2");
    await bulk.getByRole("button", { name: "保存", exact: true }).click();
    await page.getByRole("dialog").waitFor({ state: "detached" });
    let p = await readProject(4);
    assert.equal(
      p.tasks
        .filter((t) => [1, 2].includes(t.id))
        .every((t) => t.assignee === 2),
      true,
    );
    checks.push(
      "Multi-row attribute editor, forbidden title bulk editing, assignee persisted",
    );
    const desc = await col("説明");
    await cell(0, desc).scrollIntoViewIfNeeded();
    await cell(0, desc).click();
    await page.evaluate(() =>
      navigator.clipboard.writeText('"ブラウザー,\"\"引用\"\"\n複数行"\r\n'),
    );
    await page.keyboard.press("Control+v");
    await waitSave();
    assert.match(
      await cell(0, desc).textContent(),
      /ブラウザー,"引用"\r?\n複数行/,
    );
    checks.push("Quoted multiline TSV paste through actual Ctrl+V");
    await cell(0, 0).scrollIntoViewIfNeeded();
    const before = await page
      .locator("tbody tr[data-id]")
      .first()
      .getAttribute("data-id");
    const moved = await page
      .locator("tbody tr[data-id]")
      .nth(1)
      .getAttribute("data-id");
    await cell(1, 0).dragTo(cell(0, 0));
    await page.waitForFunction(
      (id) => document.querySelector("tbody tr[data-id]")?.dataset.id === id,
      moved,
    );
    assert.notEqual(before, moved);
    checks.push("Real HTML drag/drop reorders ID rows and persists");
    const titleCol = await col("件名（個別）");
    const handle = page.locator(`th[data-col="${titleCol}"] .col-resize`);
    await handle.scrollIntoViewIfNeeded();
    let box = await handle.boundingBox();
    const oldWidth = await page
      .locator(`th[data-col="${titleCol}"]`)
      .evaluate((e) => e.getBoundingClientRect().width);
    await page.mouse.move(box.x + 3, box.y + 10);
    await page.mouse.down();
    await page.mouse.move(box.x + 65, box.y + 10, { steps: 8 });
    await page.mouse.up();
    assert.equal(
      Math.round(
        (await page
          .locator(`th[data-col="${titleCol}"]`)
          .evaluate((e) => e.getBoundingClientRect().width)) - oldWidth,
      ) > 40,
      true,
    );
    await cell(0, 0).scrollIntoViewIfNeeded();
    const rowHandle = cell(0, 0).locator(".row-resize");
    box = await rowHandle.boundingBox();
    const oldHeight = await cell(0, 0).evaluate(
      (e) => e.getBoundingClientRect().height,
    );
    await page.mouse.move(box.x + 20, box.y + 3);
    await page.mouse.down();
    await page.mouse.move(box.x + 20, box.y + 40, { steps: 8 });
    await page.mouse.up();
    assert.equal(
      (await cell(0, 0).evaluate((e) => e.getBoundingClientRect().height)) >
        oldHeight + 20,
      true,
    );
    checks.push("Actual column and row boundary pointer resizing");
    await page.locator(`th[data-col="${team}"]`).scrollIntoViewIfNeeded();
    await page.locator(`th[data-col="${team}"]`).click({ button: "right" });
    await page
      .locator("#context-menu")
      .getByRole("menuitem", { name: "ブラウザー連携", exact: true })
      .click();
    await page.waitForFunction(
      () => document.querySelectorAll("tbody tr[data-id]").length === 2,
    );
    checks.push("Header right-click value filtering");
    await page.locator("#export").click();
    const csvDialog = page.getByRole("dialog");
    for (const input of await csvDialog.locator("input[data-key]").all()) {
      const key = await input.getAttribute("data-key");
      await input.setChecked(["title", "inquiry.exchange_log"].includes(key));
    }
    const download = page.waitForEvent("download");
    await csvDialog.getByRole("button", { name: "CSVを保存" }).click();
    const file = await download;
    await file.saveAs(path.join(out, "inquiries.csv"));
    const csv = fs.readFileSync(path.join(out, "inquiries.csv"), "utf8");
    assert.match(csv, /やりとり/);
    assert.match(csv, /調査中/);
    checks.push(
      "Selected-column filtered CSV browser download with exchange history",
    );
    await Promise.all([
      waitAPI("project"),
      page.locator("#clear-filter").click(),
    ]);
    await page.locator("#search").fill("ブラウザー");
    await page.locator("#search").press("Enter");
    await page.waitForFunction(
      () =>
        document.querySelectorAll("tbody tr[data-id]").length >= 1 &&
        document.querySelectorAll("tbody tr[data-id]").length <= 2,
    );
    await Promise.all([
      waitAPI("project"),
      page.locator("#clear-filter").click(),
    ]);
    await cell(0, 2).click();
    await page.locator("#exchange").click();
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "記録を追加", exact: true })
      .click();
    let editor = page.getByRole("dialog").last();
    await editor
      .getByLabel("やりとりの内容", { exact: true })
      .fill("ブラウザーの回答記録\n第二行");
    await editor.getByRole("button", { name: "保存", exact: true }).click();
    await page.waitForFunction(() =>
      Array.from(document.querySelectorAll(".log-entry p")).some((e) =>
        e.textContent.includes("ブラウザーの回答記録"),
      ),
    );
    await closeDialog();
    checks.push("Exchange log addition via human form");
    for (const label of [
      "バグ一覧",
      "課題一覧",
      "依頼・質問一覧",
      "暫定対応一覧",
    ]) {
      await nav(label);
      await page.waitForFunction(
        () => document.querySelectorAll("tbody tr[data-id]").length === 6,
      );
      await page.locator("#add").click();
      const form = page.getByRole("dialog");
      await form.getByLabel("件名", { exact: true }).fill("Web登録：" + label);
      if (label === "依頼・質問一覧")
        await form.getByLabel("相手チーム", { exact: true }).fill("Webチーム");
      if (label === "暫定対応一覧")
        await form
          .getByLabel("発生理由", { exact: true })
          .fill("製品修正を待つため");
      await form.getByRole("button", { name: "保存", exact: true }).click();
      await page.getByRole("dialog").waitFor({ state: "detached" });
      await page.waitForFunction(
        () => document.querySelectorAll("tbody tr[data-id]").length === 7,
      );
    }
    checks.push(
      "Create bug, issue, inquiry and workaround through dedicated human forms",
    );
    for (const label of [
      "バグ統計",
      "課題統計",
      "依頼・質問統計",
      "暫定対応統計",
    ]) {
      await nav(label);
      await page.locator(".chart-summary").waitFor();
      await page.locator(".bar-row").first().waitFor();
      assert.equal((await page.locator("svg").count()) > 0, true);
    }
    await page.screenshot({
      path: path.join(out, "workaround-statistics.png"),
      fullPage: true,
    });
    checks.push("All four dedicated grids and daily/classification dashboards");
    await nav("マスタ");
    await page
      .locator("#subtabs")
      .getByRole("button", { name: "担当者", exact: true })
      .click();
    await cell(0, 1).waitFor();
    await cell(0, 1).click();
    await page.keyboard.press("F2");
    await page.locator(".cell-editor").fill("ブラウザー担当");
    await page.keyboard.press("Enter");
    await waitSave();
    assert.match(await cell(0, 1).textContent(), /ブラウザー担当/);
    assert.equal(await page.locator("th").count(), 3);
    checks.push("Exclusive master tabs with native-style inline editing");
    await project(1);
    await nav("タスク一覧");
    await cell(0, 2).click({ button: "right" });
    await page
      .locator("#context-menu")
      .getByRole("menuitem", { name: "子タスクを追加", exact: true })
      .click();
    editor = page.getByRole("dialog");
    await editor
      .getByLabel("件名", { exact: true })
      .fill("Webから追加した子タスク");
    await editor.getByRole("button", { name: "保存", exact: true }).click();
    await page.getByRole("dialog").waitFor({ state: "detached" });
    p = await readProject(1);
    assert.equal(
      p.tasks.find((t) => t.title === "Webから追加した子タスク").parent,
      1,
    );
    checks.push("Right-click child task action and hierarchy persistence");
    await nav("カンバン");
    const card = page.locator('.lane[data-status="todo"] .card').first();
    const cardId = await card.getAttribute("data-id");
    await card.dragTo(page.locator('.lane[data-status="doing"] h2'));
    await page
      .locator(`.lane[data-status="doing"] .card[data-id="${cardId}"]`)
      .waitFor();
    p = await readProject(1);
    assert.equal(p.tasks.find((t) => t.id === Number(cardId)).status, "doing");
    await page.screenshot({
      path: path.join(out, "kanban.png"),
      fullPage: true,
    });
    checks.push("Real card drag/drop between status lanes");
    await nav("ガント");
    await page.locator(".gantt-bar").first().waitFor();
    assert.equal(
      await page.getByRole("img", { name: "ガント・年・月・日・曜日" }).count(),
      1,
    );
    assert.equal(await page.locator('[aria-label="稲妻線"]').count(), 1);
    const divider = page.locator(".gantt-divider");
    box = await divider.boundingBox();
    await page.mouse.move(box.x + 3, box.y + 20);
    await page.mouse.down();
    await page.mouse.move(box.x + 70, box.y + 20, { steps: 8 });
    await page.mouse.up();
    await page.screenshot({
      path: path.join(out, "gantt.png"),
      fullPage: true,
    });
    checks.push(
      "Four-level Gantt, visible lightning line and draggable name/date divider",
    );
    await nav("依存ネットワーク");
    await page.locator(".network-node").first().click();
    assert.equal((await page.locator(".network-muted").count()) > 0, true);
    checks.push("Dependency family focus dims unrelated nodes");
    for (const label of ["バーンダウン", "バーンアップ"]) {
      await nav(label);
      await page.locator("svg").waitFor();
      assert.equal((await page.locator("svg line").count()) > 20, true);
    }
    await nav("依頼・質問一覧");
    await project(4);
    await page.setViewportSize({ width: 1024, height: 768 });
    await page.reload();
    await page.locator(".grid").waitFor();
    assert.equal(await page.locator("#login").isHidden(), true);
    await page.screenshot({
      path: path.join(out, "grid-1024.png"),
      fullPage: true,
    });
    checks.push("Session restore and 1024x768 scrolling layout");
    await page.locator("#logout").click();
    await page.locator("#login").waitFor({ state: "visible" });
    await page.locator("#password").fill(process.env.YYPROJECT_TEST_READONLY);
    await page.getByRole("button", { name: "ログイン", exact: true }).click();
    await page.locator("#app").waitFor({ state: "visible" });
    await page.waitForFunction(() => document.querySelector("#add").disabled);
    assert.equal(await page.locator("#add").isDisabled(), true);
    checks.push("Readonly login disables mutations");
    assert.deepEqual(errors, []);
    fs.writeFileSync(
      path.join(out, "browser.json"),
      JSON.stringify(
        {
          result: "passed",
          checks,
          viewport: [1440, 1000],
          secondaryViewport: [1024, 768],
          browser: await browser.version(),
        },
        null,
        2,
      ),
    );
    for (const c of checks) console.log("PASS browser: " + c);
  } catch (error) {
    await page.screenshot({
      path: path.join(out, "failure.png"),
      fullPage: true,
    });
    fs.writeFileSync(
      path.join(out, "failure.txt"),
      error.stack + "\n" + errors.join("\n"),
    );
    throw error;
  } finally {
    clearTimeout(deadline);
    await context.close();
    await browser.close();
  }
}
main().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
