import assert from "node:assert/strict";
import test from "node:test";
import {
  closeSidebar,
  type SidebarLayout,
  toggleSidebar,
  visiblePanes,
} from "../src/sidebar-layout.ts";

test("keeps the paper visible beside every wide-screen sidebar combination", () => {
  for (const navigation of [false, true]) {
    for (const context of [false, true]) {
      for (const compact of ["note", "navigation"] as const) {
        const state: SidebarLayout = { navigation, context, compact };
        assert.deepEqual(visiblePanes(state, true), { navigation, context, note: true });
        assert.deepEqual(
          toggleSidebar(toggleSidebar(state, "navigation", true), "navigation", true),
          state,
        );
        assert.deepEqual(
          toggleSidebar(toggleSidebar(state, "context", true), "context", true),
          state,
        );
      }
    }
  }
});

test("keeps compact navigation preferences independent of modal details", () => {
  for (const compact of ["note", "navigation"] as const) {
    const state: SidebarLayout = { navigation: true, context: false, compact };
    assert.equal(Object.values(visiblePanes(state, false)).filter(Boolean).length, 1);
    const changed = toggleSidebar(state, "navigation", false);
    assert.equal(changed.navigation, state.navigation);
    assert.equal(changed.compact, compact === "navigation" ? "note" : "navigation");
    assert.equal(closeSidebar(changed, "navigation", false).compact, "note");
    const details = toggleSidebar(state, "context", false);
    assert.equal(details.compact, compact);
    assert.equal(details.navigation, state.navigation);
    assert.equal(details.context, true);
    assert.equal(visiblePanes(details, false).context, true);
    assert.deepEqual(closeSidebar(details, "context", false), state);
  }
});
