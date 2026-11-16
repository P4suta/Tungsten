export type SidebarSide = "navigation" | "context";
export type SidebarPane = "note" | "navigation";

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
        context: state.context,
        note: state.compact !== "navigation",
      };
}

export function toggleSidebar(
  state: SidebarLayout,
  side: SidebarSide,
  wide: boolean,
): SidebarLayout {
  if (side === "context") return { ...state, context: !state.context };
  return wide
    ? { ...state, [side]: !state[side] }
    : { ...state, compact: state.compact === side ? "note" : side };
}

export function closeSidebar(
  state: SidebarLayout,
  side: SidebarSide,
  wide: boolean,
): SidebarLayout {
  if (side === "context") return { ...state, context: false };
  return wide ? { ...state, [side]: false } : { ...state, compact: "note" };
}
