/**
 * build-item-catalog.mjs — reads SCUM's Table_TradeableDesc.json,
 * assigns short inventory codes (#wep01, #clo035, etc.), and exports
 * a static catalog JSON for the companion economy mod.
 *
 * Usage: node scripts/build-item-catalog.mjs
 * Output: data/turdmod-companion/economy/item-catalog.json
 */

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(__dirname, "..");

const TRADE_TABLE_PATH = join(
  REPO_ROOT,
  "..",
  "scumdump",
  "data",
  "extracted",
  "v23128915",
  "datatables",
  "SCUM",
  "Content",
  "ConZ_Files",
  "Economy",
  "Table_TradeableDesc.json"
);

const OUTPUT_DIR = join(REPO_ROOT, "data", "turdmod-companion", "economy");
const OUTPUT_PATH = join(OUTPUT_DIR, "item-catalog.json");

// Category mapping: ETradeCategory → short prefix
const CATEGORY_MAP = {
  "ETradeCategory::RangedWeapon": "wep",
  "ETradeCategory::RangedWeaponAccessories": "att",
  "ETradeCategory::MeleeWeapon": "mel",
  "ETradeCategory::Ammunition": "amm",
  "ETradeCategory::Explosives": "exp",
  "ETradeCategory::Pants": "clo",
  "ETradeCategory::Tops": "clo",
  "ETradeCategory::Headwear": "clo",
  "ETradeCategory::Helmets": "arm",
  "ETradeCategory::Jackets": "clo",
  "ETradeCategory::Shoes": "clo",
  "ETradeCategory::Gloves": "clo",
  "ETradeCategory::Backpacks": "clo",
  "ETradeCategory::FaceMasks": "clo",
  "ETradeCategory::Armour": "arm",
  "ETradeCategory::Underwear": "clo",
  "ETradeCategory::NeckWaist": "clo",
  "ETradeCategory::FirstAid": "med",
  "ETradeCategory::Food": "fod",
  "ETradeCategory::Seeds": "fod",
  "ETradeCategory::CraftingMaterial": "mat",
  "ETradeCategory::FishingEquipment": "tol",
  "ETradeCategory::Tools": "tol",
  "ETradeCategory::Electronics": "elc",
  "ETradeCategory::VehicleParts": "veh",
  "ETradeCategory::Vehicles": "veh",
  "ETradeCategory::SurvivalEquipment": "tol",
  "ETradeCategory::Miscellaneous": "msc",
  "ETradeCategory::Decoration": "msc",
  "ETradeCategory::Drinks": "fod",
};

const CATEGORY_NAMES = {
  wep: "Weapons (Ranged)",
  mel: "Melee Weapons",
  amm: "Ammo & Magazines",
  att: "Attachments",
  clo: "Clothing",
  arm: "Armor & Helmets",
  med: "Medical",
  fod: "Food & Drink",
  mat: "Materials",
  tol: "Tools",
  elc: "Electronics",
  veh: "Vehicles & Parts",
  exp: "Explosives",
  msc: "Misc",
};

console.log("Reading trade table...");
const raw = JSON.parse(readFileSync(TRADE_TABLE_PATH, "utf-8"));
const rows = raw[0]?.Rows;
if (!rows) {
  console.error("Could not find Rows in trade table");
  process.exit(1);
}

const entries = Object.entries(rows);
console.log(`Found ${entries.length} tradeable items`);

// Group by prefix
const groups = {};
for (const [key, row] of entries) {
  const cat = row.TradeCategory || "ETradeCategory::Miscellaneous";
  const prefix = CATEGORY_MAP[cat] || "msc";
  if (!groups[prefix]) groups[prefix] = [];

  const displayName =
    row.TradingEntryCaption?.SourceString ||
    row.TradingEntryCaption?.LocalizedString ||
    key.replace(/_C$/, "").replace(/_/g, " ");

  const assetPath = row.TradeableClass?.AssetPathName || "";
  const spawnRowKey = key; // e.g. "Weapon_AK47_C"
  const spawnCode = key.replace(/_C$/, ""); // e.g. "Weapon_AK47"

  groups[prefix].push({
    key,
    displayName,
    category: cat,
    assetPath,
    spawnRowKey,
    spawnCode,
    traderBuyPrice: row.BasePurchasePriceModifier ?? 1.0,
    requiredFame: row.RequiredFamePoints ?? 0,
    canBuy: row.CanBePurchasedByPlayer ?? false,
    canSell: row.CanBeSoldByPlayer ?? false,
  });
}

// Sort each group alphabetically and assign numbers
const items = {};
const bySlug = {};
const bySpawnCode = {};
const categories = {};

for (const [prefix, group] of Object.entries(groups)) {
  group.sort((a, b) => a.displayName.localeCompare(b.displayName));

  const padWidth = group.length >= 100 ? 3 : 2;
  categories[prefix] = {
    name: CATEGORY_NAMES[prefix] || prefix,
    count: group.length,
  };

  for (let i = 0; i < group.length; i++) {
    const num = String(i + 1).padStart(padWidth, "0");
    const shortCode = `${prefix}${num}`;
    const item = group[i];

    items[shortCode] = {
      slug: item.key.toLowerCase().replace(/_c$/, ""),
      displayName: item.displayName,
      category: CATEGORY_NAMES[prefix] || prefix,
      tradeCategory: item.category,
      assetPath: item.assetPath,
      spawnCode: item.spawnCode,
      traderBuyPrice: item.traderBuyPrice,
      requiredFame: item.requiredFame,
      canBuy: item.canBuy,
      canSell: item.canSell,
    };

    bySlug[items[shortCode].slug] = shortCode;
    bySpawnCode[item.spawnCode] = shortCode;
  }
}

const catalog = {
  version: "23128915",
  generatedAt: new Date().toISOString(),
  totalItems: Object.keys(items).length,
  items,
  bySlug,
  bySpawnCode,
  categories,
};

mkdirSync(OUTPUT_DIR, { recursive: true });
writeFileSync(OUTPUT_PATH, JSON.stringify(catalog, null, 2));

console.log(`\nCatalog written to ${OUTPUT_PATH}`);
console.log(`Total items: ${catalog.totalItems}`);
console.log("\nCategories:");
for (const [prefix, info] of Object.entries(categories)) {
  console.log(`  ${prefix}: ${info.name} (${info.count} items)`);
}

// Show some examples
console.log("\nSample codes:");
const samples = ["wep01", "wep02", "mel01", "amm01", "clo001", "med01", "fod01", "att01", "exp01"];
for (const code of samples) {
  const item = items[code];
  if (item) console.log(`  #${code} = ${item.displayName} (${item.spawnCode})`);
}
