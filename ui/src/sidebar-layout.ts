export type SidebarSide = "navigation" | "context";
export type SidebarPane = "note" | SidebarSide;

export interface SidebarLayout {
  readonly navigation: boolean;
  readonly context: boolean;
  readonly compact: SidebarPane;
}

export interface SidebarVisibility {
  readonly navigation: boolean;
  readonly context: boolean;
  readonly note: boolean;
}

export function visiblePanes(state: SidebarLayout, wide: boolean): SidebarVisibility {
  return wide
    ? { navigation: state.navigation, context: state.context, note: true }
    : {
        navigation: state.compact === "navigation",
        context: state.compact === "context",
        note: state.compact === "note",
      };
}

export function toggleSidebar(
  state: SidebarLayout,
  side: SidebarSide,
  wide: boolean,
): SidebarLayout {
  return wide
    ? { ...state, [side]: !state[side] }
    : { ...state, compact: state.compact === side ? "note" : side };
}

export function closeSidebar(
  state: SidebarLayout,
  side: SidebarSide,
  wide: boolean,
): SidebarLayout {
  return wide ? { ...state, [side]: false } : { ...state, compact: "note" };
}
