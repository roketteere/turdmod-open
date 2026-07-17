/**
 * `turdmod.ui` — HUD widgets + custom menus + input.
 *
 * Strategy C only (in-process). Strategy B never gets UI hooks; mods that
 * need both should split their UI into a Strategy D companion app.
 *
 * @since 1.0
 */

import { type Disposable, requireNamespace, TurdMODError, type RGBA } from "./runtime.js";

export interface HudTextWidget {
  id: string;
  text: string;
  /** Anchor on the screen, normalized [0..1] with origin top-left. */
  anchor: { x: number; y: number };
  fontPx?: number;
  color?: RGBA;
  visible?: boolean;
}

export interface MenuItem {
  id: string;
  label: string;
  /** Optional submenu. Either action or items, not both. */
  items?: MenuItem[];
  action?: () => void;
  enabled?: boolean;
}

export interface CustomMenu {
  id: string;
  title: string;
  items: MenuItem[];
}

/** Standard SCUM input bindings: keyboard chord like "Ctrl+M", "F8". */
export type Hotkey = string;

interface UiRuntime {
  setHud(widget: HudTextWidget): Disposable;
  removeHud(id: string): boolean;

  showMenu(menu: CustomMenu): Disposable;
  closeMenu(id: string): boolean;

  /** Returns disposable that unregisters the binding. */
  bindHotkey(key: Hotkey, handler: () => void): Disposable;

  /** Brief on-screen toast. Optional duration; default 3000ms. */
  toast(text: string, durationMs?: number): void;
}

function rt(): UiRuntime {
  return requireNamespace<UiRuntime>("ui");
}

/**
 * Render or update a HUD text widget. The widget keeps its id; calling with
 * the same id replaces the existing widget atomically (no flicker).
 *
 * @since 1.0
 */
export function setHud(widget: HudTextWidget): Disposable {
  if (!widget.id || typeof widget.text !== "string") {
    throw new TurdMODError("setHud: id and text are required", "BAD_WIDGET");
  }
  if (!widget.anchor || widget.anchor.x < 0 || widget.anchor.x > 1 || widget.anchor.y < 0 || widget.anchor.y > 1) {
    throw new TurdMODError("setHud: anchor.x and anchor.y must be in [0, 1]", "BAD_ANCHOR");
  }
  return rt().setHud(widget);
}

/** @since 1.0 */
export const removeHud = (id: string): boolean => rt().removeHud(id);

/**
 * Open a custom radial / list menu. Returns disposable; dispose to close.
 *
 * @since 1.0
 */
export function showMenu(menu: CustomMenu): Disposable {
  if (!menu.id || !menu.title || !Array.isArray(menu.items)) {
    throw new TurdMODError("showMenu: id, title, and items are required", "BAD_MENU");
  }
  return rt().showMenu(menu);
}

/** @since 1.0 */
export const closeMenu = (id: string): boolean => rt().closeMenu(id);

/**
 * Bind a global hotkey. Hotkeys are mod-scoped — collisions across mods are
 * resolved by load order (last-loaded wins) and surfaced in the manager UI.
 *
 * @since 1.0
 */
export function bindHotkey(key: Hotkey, handler: () => void): Disposable {
  if (!key) throw new TurdMODError("bindHotkey: key is required", "BAD_KEY");
  return rt().bindHotkey(key, handler);
}

/** @since 1.0 */
export const toast = (text: string, durationMs = 3000): void => rt().toast(text, durationMs);
