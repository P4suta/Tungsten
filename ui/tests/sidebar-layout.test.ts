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
      for (const compact of ["note", "navigation", "context"] as const) {
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

test("shows exactly one compact pane without replacing desktop preferences", () => {
  for (const compact of ["note", "navigation", "context"] as const) {
    const state: SidebarLayout = { navigation: true, context: false, compact };
    assert.equal(Object.values(visiblePanes(state, false)).filter(Boolean).length, 1);
    for (const side of ["navigation", "context"] as const) {
      const changed = toggleSidebar(state, side, false);
      assert.equal(changed.navigation, state.navigation);
      assert.equal(changed.context, state.context);
      assert.equal(changed.compact, compact === side ? "note" : side);
      assert.equal(closeSidebar(changed, side, false).compact, "note");
    }
  }
});
