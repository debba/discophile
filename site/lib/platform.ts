import type { Platform } from "./releases";

type Device = { userAgent: string; maxTouchPoints: number; userAgentData?: { platform?: string } };

export function detectPlatform(device: Device): Platform | null {
  const agent = device.userAgent.toLowerCase();
  if (/iphone|ipad|ipod/.test(agent)) return "ios";
  if (agent.includes("android")) return "android";
  const name = (device.userAgentData?.platform || agent).toLowerCase();
  // iPadOS can report macOS, but unlike a Mac it has a touch screen.
  if (name.includes("mac")) return device.maxTouchPoints > 1 ? "ios" : "mac";
  if (name.includes("win")) return "windows";
  if (name.includes("linux") || name.includes("x11")) return "linux";
  return null;
}
