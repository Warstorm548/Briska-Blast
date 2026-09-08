# Loot table & the Full Barrier

Shipped in game **0.35.0** / server **0.36.0** / shared **0.7.0**.

The hotbar shipped in 0.27.0 as an empty container — five keybound slots and no items. This is
the item system that fills it, plus its first item: a collectible that deploys a barrier across
the player's goal mouth.

**Read this before tuning anything.** Every knob is listed in one table below, with where it
lives and what visibly changes when you move it.

---

## 1. Every tunable

### Host-facing (Host Game → **Loot Table** tab)

These travel to the server, are validated there, and are broadcast to every client, so the whole
table plays by the host's numbers.

| Setting | Range | Default | Effect |
|---|---|---|---|
| Drop Rate | 5–60s | **20s** | How often the table is *rolled*. A roll can produce nothing. |
| In Loot Table | on/off | **on** | Off removes the item entirely — it can never drop. |
| Drop Chance | 1–100 | **50** | The item's weight. See §2 — it is a literal percentage only while no other item shares it. |
| Duration | 5–120s | **5s** | Seconds one activation adds to the barrier timer. Defaults to the floor of its range. |

Bounds live once, in `shared/src/types/loot_settings.rs`, and are **hand-mirrored** into
`LootSettingsDto` (`client/src/net/Dto.cs`). Change one, change the other — there is no codegen.
This is the same contract `SpawnSettings` and `WinCondition` already use.

### Code-level constants

Not host-configurable; change these in code and re-release.

| Constant | File | Value | Effect |
|---|---|---|---|
| `ShieldClearanceHFrac` | `client/src/game/GameScene.cs` | 6/1440 of arena height | Gap between the paddle's underside and the barrier. Below ~4px they read as one object. |
| `ShieldThicknessHFrac` | `GameScene.cs` | 30/1440 | Barrier thickness. The whole goal gap is only 120/1440, so this is roughly a quarter of it. |
| `ShieldEndClearanceHFrac` | `GameScene.cs` | 4/1440 | How far the bar's ends stop short of the corner barriers. **Do not raise this far** — see §3. |
| `MaxStack` | `client/src/game/ItemRegistry.cs` | 5 | Charges one slot holds. |
| pickup radius | `GameScene.TickLoot` | `_ballRadius * 1.5f` | Collectible size, matching the splitter. |
| spawn margin | `GameScene.TickLoot` | `_ballRadius * 4f` | Keeps drops off the edges and out of the paddle band. |
| one-at-a-time cap | `GameScene.TickLoot` | `Pickups.Count > 0` → skip | At most one uncollected pickup per screen. Remove this line to allow pile-up. |
| `ItemCount` | `Dto.cs` + `loot_settings.rs` | 1 | Number of items in the table. Bumped when adding one — see §5. |

---

## 2. How the weighting actually works

**A weight is a bucket, and items sharing a weight share that bucket.**

The odds are computed over the **distinct** weights of enabled items. Items tied on a weight
split their bucket evenly. Whatever is left of 100 is the chance nothing drops.

| Enabled items | Distinct total | Resulting rates | Nothing |
|---|---|---|---|
| Barrier 50, alone *(the default)* | 50 | Barrier **50%** | 50% |
| Barrier 50, item B 50 | **50** — one 50, not two | 25% / 25% | 50% |
| Barrier 10, B 50, C 50 | 60 | 10% / 25% / 25% | 40% |
| Barrier 40, B 40, C 20 | 60 | 20% / 20% / 20% | 40% |

> **The counterintuitive part.** A weight stops being a literal percentage the moment a second
> item shares it. Putting a second item on 50 does **not** leave both at 50% — it makes each 25%.
> The number means "this tier fires 50% of the time", split among whoever is in the tier.

That is why the Loot Table tab shows each item's **resolved rate** and a live "Nothing X%",
rather than echoing the raw weights back. A host sees the split happen while setting it.

**The 100% cap.** The distinct total can never exceed 100. Enforced in the host UI (Create is
blocked with an explanatory message) and again server-side (`invalid_loot_settings`, naming the
offending field). With a single item this is unreachable, since its own slider caps at 100.

Note one subtlety for when item #2 lands: because a tied weight adds **zero** to the total,
"shrink each slider to the remaining headroom" would be *wrong* — it would forbid legal tied
values. The implementation therefore leaves sliders at their full range and validates the total.

**Where it is tested:** `shared/src/types/loot_settings.rs`, run with `cargo test -p shared`.
That is the reference implementation; `LootSettingsDto` and `LootTable.cs` mirror it, and the C#
side has no test runner. **CI does not run shared's tests** (`ci-server.yml` runs only
`cargo build -p server` and `cargo test -p server`) — run them by hand.

---

## 3. Geometry

### The icon — why the interior is 84, not 90

The hotbar slot sprite is **96×96** with a **6px** frame. The frame is subtracted from **both**
sides, so the usable interior is `96 − 6 − 6 = 84`, not 90. This has bitten before. Any icon is
authored at 84×84 and positioned by anchor insets of `IconInsetFrac = 6/96`, so a square sprite's
size is implied rather than stored separately.

`BarrierShieldIcon.png` is 84×84: a heater shield (52×56, flat top tapering to a point, bounded by
two circular arcs each centred on the opposite top corner) flanked by two r=11 semicircles, with a
measured **9.0px** minimum gap between them.

### The barrier — why the ends are computed, never hardcoded

The barrier is a **capsule**: a spine segment plus a radius, which is exactly "a rectangle with
half-round ends". The sim collides against the spine (`GameSimulation.ResolveShield`), so the
rounded ends fall out for free — near an end the closest point on the segment *is* the endpoint.

Its ends are solved at runtime in `GameScene.ResolveShieldGeometry`, by bisecting on the real
`CornerBarrier.ClosestPoint` until the capsule clears the corner triangles. **Clients run
different arena aspect ratios**, so a baked inset would be correct on exactly one screen. This
also preserves CornerBarrier's standing invariant that art and collider derive from one source.

Verified spans:

| Resolution | Arena | Barrier y | Barrier x | % of width | End gap | Ball ⌀ |
|---|---|---|---|---|---|---|
| 2560×1440 | 2560×1344 | 1271–1299 | 66 → 2492 | 94.8% | 3.7px | 44.8px |
| 1920×1080 | 1920×1008 | 953–974 | 50 → 1869 | 94.8% | 2.8px | 33.6px |
| 3440×1440 (21:9) | 3440×1344 | 1271–1299 | 66 → 3372 | 96.1% | 3.7px | 44.8px |
| 1600×1200 (4:3) | 1600×1120 | 1059–1083 | 55 → 1544 | 93.0% | 3.1px | 37.3px |

**The end gap is load-bearing.** It is roughly a tenth of a ball's diameter, so the barrier and
the corner triangles seal the goal *between* them while never overlapping. Raising
`ShieldEndClearanceHFrac` past about half a ball diameter opens a hole balls can score through.

### Rendering

`FullBarrier.png` is **240×64**: a 60px-thick capsule with a 2px transparent margin top and
bottom so the antialiased edge is not clipped. It is drawn as **three region-sliced sprites** —
`cap | middle | cap`, at x 0–32, 32–208, 208–240.

Stretching one sprite across ~2400px would smear the rounded caps into ellipses, and the caps are
the shape's whole point. So the caps scale **uniformly** and only the middle stretches — which is
lossless because the source's middle region has verified **zero** horizontal variation.

---

## 4. Who gets the item, and the award flow

**The ball's last hitter collects it — not the player whose screen it spawned on.** You knock the
ball into the shield, you earn it.

Loot spawns per-screen and local-only, exactly like `BallSpliter`. But balls cross portals
carrying their `LastHitterId`, so the earner is frequently a *remote peer* — which makes the
award the one part of this feature that leaves the screen.

Three cases, resolved in `GameSimulation.ResolvePickups`:

| Ball's `LastHitterId` | Outcome |
|---|---|
| empty (nobody has hit it) | **Pickup is left on the field.** Nobody earned it. |
| the local player | Awarded locally. |
| a peer | `ItemAward` packet sent to that peer; pickup consumed. |

**The packet.** `GameMsg.ItemAward = 2` in `client/src/game/net/GamePacket.cs` — discriminator
byte plus a 4-byte item id, sent directly peer-to-peer over the same data channel ball handoffs
already use. Not server-relayed: a *reward* does not warrant being made more reliable than the
balls themselves.

**Two accepted failure modes.** Both are deliberate:
- **Closed channel** → the award is lost and logged as a warning, exactly as a lost handoff is.
- **Full stack** → the pickup is consumed and the item wasted. The awarding screen cannot see a
  remote player's hotbar, so "put it back when the collector is full" is not implementable
  symmetrically without an ack round trip per pickup.

**Trust.** A peer can now tell you "you earned an item". The recipient enforces its own
`MaxStack`, so the worst a hostile peer achieves is filling your hotbar with items you could have
collected anyway — no new authority, given a client already reports its own goals.

**Back-compat.** `TryReadBallHandoff` rejects unknown discriminators, so a 0.34.1 client discards
an `ItemAward` frame without crashing. It simply never receives items it earned while still
awarding them to others — a silent one-way failure, which is why `min_game_version` must be
raised to 0.35.0.

---

## 5. Adding loot item #2

1. **Art** — drop the PNG under `client/src/assets/sprites/loottable/`, then run
   `scripts/godot-headless.sh --headless --import` from `client/` to generate the `.png.import`
   sidecar. **Then re-normalise indentation**: the import rewrites C# files from 4 spaces to tabs,
   including files you never touched.
2. **`AssetId`** — next free number in `client/src/core/SpriteRegistry.cs`, plus a row in
   `Entries`. Category `SystemHandled` for a collectible.
3. **`ItemRegistry`** — an `ItemId`, a row in `Entries` (display name, icon, max stack), and an
   entry in `LootOrder`.
4. **`LootSettings`** (Rust) — bump `LOOT_ITEM_COUNT`, add the three fields
   (`<item>_enabled`, `<item>_weight`, plus whatever the item's own knob is), add its pair to
   `entries()`, and extend `validate()`. Add tests.
5. **`LootSettingsDto`** (C#) — mirror all of the above, including `ItemCount`.
   `ItemRegistry.LootOrder` is sized by `LootSettingsDto.ItemCount`, so a mismatch is a **compile
   error**, not a silently mis-assigned drop weight.
6. **Host UI** — one more row group in the `LootTable` tab of `HostSetupMenu.tscn`, wired in
   `HostSetupMenu.cs`. The tab scrolls, so there is room.
7. **Effect** — a case in `GameScene.OnHotbarSlotActivated`, and whatever state it needs on
   `GameState`.

The weighting maths, the roll, the pickup spawner and the award routing are all already generic
over N items — none of them need touching.

---

## 6. Accepted behaviours & known edges

- **A ball already below the barrier line when it deploys still scores.** The barrier does not
  retroactively save; it appears where it appears.
- **A pickup earned at a full stack is wasted** (see §4).
- **An award can be lost to a closed peer channel** (see §4).
- **Drops are rolled independently per screen**, so two players can see different loot in the same
  second. The odds are identical; the outcomes are not. Same model as the ball splitter.
- **Deflecting off the barrier does not change ball ownership.** It behaves like the corner
  barriers, not like the paddle — the attacker keeps the ball, so a barrier's value is bounded to
  its duration rather than permanently defusing a ball.
- **Stacked time is capped only by the stack**: 5 charges × up to 120s = up to 600s.
- **The barrier blocks balls, not other players.** Nothing else in the arena interacts with it.

---

## Related

- [`extended-mode.md`](extended-mode.md) — the mode this plays inside, and the BallSpliter
  spawner this is modelled on.
- [`asset-registry.md`](asset-registry.md) — adding a sprite.
- [`protocol.md`](protocol.md) — why C# hand-mirrors the Rust wire types.
