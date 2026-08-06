import type { Metadata, Viewport } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: {
    default: "Dungeon Barrage",
    template: "%s · Dungeon Barrage",
  },
  description:
    "Build your fighter, bend the wind, and blast apart a living dungeon in a turn-based artillery duel.",
  applicationName: "Dungeon Barrage",
  manifest: "/manifest.webmanifest",
};

export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  themeColor: "#120f1c",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
