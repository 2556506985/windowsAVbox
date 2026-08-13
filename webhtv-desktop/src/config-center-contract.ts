export const CONFIG_SELECT_OPTIONS = {
  aliHD: ["阿里原画", "阿里普画", "阿里原画|阿里普画", "阿里普画|阿里原画"],
  aliQuality: ["阿里原画", "阿里普画", "阿里原画|阿里普画", "阿里普画|阿里原画"],
  quarkHD: ["夸克原画", "夸克普画", "夸克无限", "夸克原画|夸克普画", "夸克普画|夸克原画"],
  quarkQuality: ["夸克原画", "夸克普画", "夸克无限", "夸克原画|夸克普画", "夸克普画|夸克原画"],
  ucHD: ["UC原画", "UC普画", "UC无限", "UC原画|UC普画", "UC普画|UC原画"],
  ucQuality: ["UC原画", "UC普画", "UC无限", "UC原画|UC普画", "UC普画|UC原画"],
  baiduHD: ["百度原画", "百度无限"],
  baiduQuality: ["百度原画", "百度无限"],
  "123HD": ["123原画", "123无限"],
  "123Quality": ["123原画", "123无限"],
  aliThread: ["4", "8", "16", "32", "64", "128", "256"],
  quarkThread: ["4", "8", "16", "32", "64", "128", "256"],
  ucThread: ["自动", "4", "8", "16", "32", "64", "128", "256"],
  baiduThread: ["4", "6", "8", "10", "12", "14", "16"],
  xunleiThread: ["4", "6", "8", "10", "12", "14", "16"],
  danmuColor: ["默认", "彩色"],
  proxyMode: ["Java多线程"],
  update: ["开启", "关闭"],
} as const;

export const PAN_BLOCK_PROVIDERS = {
  BlockAli: "阿里云盘",
  BlockQuark: "夸克网盘",
  BlockUC: "UC网盘",
  Block189: "天翼云盘",
  Block123: "123云盘",
  BlockBaidu: "百度网盘",
  BlockXunlei: "迅雷云盘",
  BlockGuangya: "光鸭云盘",
} as const;

export const DEFAULT_PAN_ORDER = ["夸克", "UC", "百度", "迅雷", "光鸭", "天翼", "123", "阿里"] as const;

export type ConfigSelectKey = keyof typeof CONFIG_SELECT_OPTIONS;
export type CloudAuthProvider = "quark" | "uc" | "uctv" | "baidu";

export type ConfigCenterAction =
  | { kind: "select"; key: ConfigSelectKey; options: readonly string[] }
  | { kind: "pan-block"; provider: string }
  | { kind: "pan-order"; key: "panOrder" }
  | { kind: "auth-login"; provider: CloudAuthProvider }
  | { kind: "auth-clear"; provider: CloudAuthProvider }
  | { kind: "unsupported" };

export function classifyConfigAction(action?: string | null): ConfigCenterAction {
  const normalized = action?.trim() || "";
  if (Object.prototype.hasOwnProperty.call(CONFIG_SELECT_OPTIONS, normalized)) {
    const key = normalized as ConfigSelectKey;
    return { kind: "select", key, options: CONFIG_SELECT_OPTIONS[key] };
  }
  if (Object.prototype.hasOwnProperty.call(PAN_BLOCK_PROVIDERS, normalized)) {
    const key = normalized as keyof typeof PAN_BLOCK_PROVIDERS;
    return { kind: "pan-block", provider: PAN_BLOCK_PROVIDERS[key] };
  }
  if (normalized === "panOrder") return { kind: "pan-order", key: "panOrder" };
  if (normalized === "300" || normalized === "newquark") return { kind: "auth-login", provider: "quark" };
  if (normalized === "500" || normalized === "newuc") return { kind: "auth-login", provider: "uc" };
  if (normalized === "uctoken") return { kind: "auth-login", provider: "uctv" };
  if (normalized === "600token") return { kind: "auth-clear", provider: "uctv" };
  if (normalized === "b300" || normalized === "newbaidu") return { kind: "auth-login", provider: "baidu" };
  if (normalized === "400") return { kind: "auth-clear", provider: "quark" };
  if (normalized === "600") return { kind: "auth-clear", provider: "uc" };
  if (normalized === "b400") return { kind: "auth-clear", provider: "baidu" };
  return { kind: "unsupported" };
}

export function parsePanBlock(value?: string | null): string[] {
  return uniqueCommaValues(value ? value.split(",") : []);
}

export function togglePanBlock(value: string | null | undefined, provider: string, enabled: boolean): string {
  const providers = parsePanBlock(value);
  const normalizedProvider = provider.trim();
  if (!normalizedProvider) return providers.join(",");
  const next = enabled
    ? providers.filter((item) => item !== normalizedProvider)
    : [...providers, normalizedProvider];
  return uniqueCommaValues(next).join(",");
}

export function normalizePanOrder(value?: string | readonly string[] | null): string[] {
  const source = typeof value === "string" ? value.split(",") : value || [];
  const order = uniqueCommaValues(source);
  for (const provider of DEFAULT_PAN_ORDER) {
    if (!order.includes(provider)) order.push(provider);
  }
  return order;
}

export function movePanOrder(
  value: string | readonly string[] | null | undefined,
  index: number,
  direction: -1 | 1,
): string[] {
  const order = normalizePanOrder(value);
  const destination = index + direction;
  if (!Number.isInteger(index) || index < 0 || destination < 0 || destination >= order.length) return order;
  [order[index], order[destination]] = [order[destination], order[index]];
  return order;
}

function uniqueCommaValues(values: readonly string[]): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const value of values) {
    const normalized = value.trim();
    if (!normalized || seen.has(normalized)) continue;
    seen.add(normalized);
    result.push(normalized);
  }
  return result;
}
