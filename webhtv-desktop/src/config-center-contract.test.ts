import { describe, expect, it } from "vitest";
import {
  CONFIG_SELECT_OPTIONS,
  classifyConfigAction,
  movePanOrder,
  normalizePanOrder,
  parsePanBlock,
  togglePanBlock,
} from "./config-center-contract";

describe("config center action contract", () => {
  it("classifies only explicitly supported select actions", () => {
    expect(classifyConfigAction("aliHD")).toEqual({
      kind: "select",
      key: "aliHD",
      options: CONFIG_SELECT_OPTIONS.aliHD,
    });
    expect(classifyConfigAction("ucThread")).toMatchObject({ kind: "select", key: "ucThread" });
    expect(CONFIG_SELECT_OPTIONS.ucThread).toContain("自动");
    expect(CONFIG_SELECT_OPTIONS.ucQuality).toContain("UC无限");
    expect(CONFIG_SELECT_OPTIONS.proxyMode).toEqual(["Java多线程"]);
  });

  it("maps block and order actions without enabling arbitrary actions", () => {
    expect(classifyConfigAction("BlockGuangya")).toEqual({ kind: "pan-block", provider: "光鸭云盘" });
    expect(classifyConfigAction("panOrder")).toEqual({ kind: "pan-order", key: "panOrder" });
    expect(classifyConfigAction("300")).toEqual({ kind: "auth-login", provider: "quark" });
    expect(classifyConfigAction("newuc")).toEqual({ kind: "auth-login", provider: "uc" });
    expect(classifyConfigAction("uctoken")).toEqual({ kind: "auth-login", provider: "uctv" });
    expect(classifyConfigAction("600token")).toEqual({ kind: "auth-clear", provider: "uctv" });
    expect(classifyConfigAction("b300")).toEqual({ kind: "auth-login", provider: "baidu" });
    expect(classifyConfigAction("400")).toEqual({ kind: "auth-clear", provider: "quark" });
    expect(classifyConfigAction("600")).toEqual({ kind: "auth-clear", provider: "uc" });
    expect(classifyConfigAction("b400")).toEqual({ kind: "auth-clear", provider: "baidu" });
    expect(classifyConfigAction("二维码登录")).toEqual({ kind: "unsupported" });
    expect(classifyConfigAction("recovery")).toEqual({ kind: "unsupported" });
    expect(classifyConfigAction("unknownAction")).toEqual({ kind: "unsupported" });
  });
});

describe("panBlock contract", () => {
  it("trims, removes empty values, and de-duplicates without dropping unknown providers", () => {
    expect(parsePanBlock(" 阿里云盘,自定义盘,,阿里云盘, 自定义盘 ")).toEqual(["阿里云盘", "自定义盘"]);
  });

  it("removes enabled providers and adds disabled providers", () => {
    expect(togglePanBlock("自定义盘,阿里云盘", "阿里云盘", true)).toBe("自定义盘");
    expect(togglePanBlock("自定义盘", "光鸭云盘", false)).toBe("自定义盘,光鸭云盘");
    expect(togglePanBlock("自定义盘,光鸭云盘", "光鸭云盘", false)).toBe("自定义盘,光鸭云盘");
  });
});

describe("panOrder contract", () => {
  it("uses the default order and completes partial orders while retaining unknown providers", () => {
    expect(normalizePanOrder("")).toEqual(["夸克", "UC", "百度", "迅雷", "光鸭", "天翼", "123", "阿里"]);
    expect(normalizePanOrder("百度,自定义,夸克,百度")).toEqual([
      "百度", "自定义", "夸克", "UC", "迅雷", "光鸭", "天翼", "123", "阿里",
    ]);
  });

  it("moves entries one position and keeps boundary moves stable", () => {
    const order = ["夸克", "自定义", "UC", "百度", "迅雷", "光鸭", "天翼", "123", "阿里"];
    expect(movePanOrder(order, 1, -1).slice(0, 3)).toEqual(["自定义", "夸克", "UC"]);
    expect(movePanOrder(order, 1, 1).slice(0, 3)).toEqual(["夸克", "UC", "自定义"]);
    expect(movePanOrder(order, 0, -1)).toEqual(order);
  });
});
