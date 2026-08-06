import type { Metadata } from "next";
import { DungeonBarrageGame } from "./game/DungeonBarrageGame";

export const metadata: Metadata = {
  title: "Dungeon Barrage — Play the vertical slice",
  description:
    "A turn-based artillery duel with destructible dungeons, expressive loadouts, and layered character customization.",
};

export default function Home() {
  return <DungeonBarrageGame />;
}
