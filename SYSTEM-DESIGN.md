# SYSTEM OVERVIEW

┌─────────────────────────────────────────────────────────────────┐
│                         SYSTEM OVERVIEW                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  1. MINT TOKENS                                                 │
│     ┌─────────────┐    memo    ┌──────────────────┐             │
│     │   User      │ ────────►  │ process_transfer │             │
│     │   Wallet    │            │                  │             │
│     └─────────────┘            │  • Check memo    │             │
│                                │  • Random amount │             │
│                                │  • Mint tokens   │             │
│                                │  • Update stats  │             │
│                                └──────────────────┘             │
│                                                                 │
│  2. BURN TOKENS                                                 │
│     ┌─────────────┐   burn +   ┌──────────────────┐             │
│     │   User      │   memo     │  process_burn    │             │
│     │   Wallet    │ ────────►  │                  │             │
│     └─────────────┘            │  • Burn tokens   │             │
│                                │  • Update stats  │             │
│                                │  • Record burn   │             │
│                                └─────┬────────────┘             │
│                                      │                          │
│                                      ▼                          │
│     ┌─────────────────┐     ┌─────────────────┐                 │
│     │ LatestBurnShard │     │  TopBurnShard   │                 │
│     │   (all burns)   │     │ (420+ tokens)   │                 │
│     └─────────────────┘     └─────────────────┘                 │
│                                                                 │
│  3. SHARD MANAGEMENT                                            │
│     ┌───────────────────────────────────────┐                   │
│     │  When TopBurnShard becomes full:      │                   │
│     │  1. Create new shard                  │                   │
│     │  2. Update GlobalTopBurnIndex         │                   │
│     │  3. Point to new active shard         │                   │
│     └───────────────────────────────────────┘                   │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘


# BURN STRUCTURE

┌────────────────────────────────────────────────────────────────┐
│                        BURN STRUCTURE                          │
├────────────────────────────────────────────────────────────────┤
│                                                                │
│  ┌─────────────────┐    ┌──────────────────┐                   │
│  │ GlobalTopBurn   │    │ LatestBurnShard  │                   │
│  │     Index       │    │   (circular)     │                   │
│  │                 │    │                  │                   │
│  │ total_count: 3  │    │ current_idx: 25  │                   │
│  │ current_idx: 1  │    │ records[69] ───┐ │                   │
│  └─────┬───────────┘    └──────────────┼─┘ │                   │
│        │                               │   │                   │
│        │ points to active shard        │   ▼                   │
│        ▼                               │ ┌─────────────────┐   │
│  ┌─────────────────┐                   │ │   BurnRecord    │   │
│  │ TopBurnShard #0 │                   │ │                 │   │
│  │     FULL ❌     │                   │ │ pubkey: [32]    │   │
│  │ records[69/69]  │                   │ │ signature: str  │   │
│  └─────────────────┘                   │ │ slot: u64       │   │
│                                        │ │ blocktime: i64  │   │
│  ┌─────────────────┐                   │ │ amount: u64     │   │
│  │ TopBurnShard #1 │ ◄─── ACTIVE       │ └─────────────────┘   │
│  │     ACTIVE ✅   │                   │                       │
│  │ records[42/69]  │                   └─ Used by both Latest  │
│  └─────────────────┘                     and Top burn shards   │
│                                                                │
│  ┌─────────────────┐                                           │
│  │ TopBurnShard #2 │                                           │
│  │    EMPTY 💤     │                                           │
│  │ records[0/69]   │                                           │
│  └─────────────────┘                                           │
│                                                                │
└────────────────────────────────────────────────────────────────┘

# GLOBAL TOP BURN INDEX 

┌─────────────────────────────────────┐
│         GlobalTopBurnIndex          │
├─────────────────────────────────────┤
│                                     │
│  top_burn_shard_total_count: u64    │  ── Total allocated shards
│ ┌─────────────────────────────────┐ │
│ │            3                    │ │
│ └─────────────────────────────────┘ │
│                                     │
│  top_burn_shard_current_index:      │  ── Points to active shard
│ ┌─────────────────────────────────┐ │
│ │ Option<u64> = Some(1)           │ │  ── None if no available shards
│ └─────────────────────────────────┘ │
│                                     │
│  Seeds: [b"global_top_burn_index"]  │
└─────────────────────────────────────┘

# TOP BURN SHARD

┌─────────────────────────────────────────────────────────────────┐
│                           TopBurnShard                          │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  index: u64 ──────────────────┐                                 │
│  creator: Pubkey              │                                 │
│  records: Vec<BurnRecord>     │                                 │
│                               │                                 │
│  ┌─── Shard #0 ──────────────┐│   ┌─── Shard #1 ──────────────┐ │
│  │ index: 0                  ││   │ index: 1                  │ │
│  │ creator: Alice            ││   │ creator: Bob              │ │
│  │ records: [69/69] FULL ❌  ││   │ records: [42/69] ACTIVE ✅│ │
│  │                           ││   │                           │ │
│  │ MIN_BURN: 420 tokens      ││   │ MIN_BURN: 420 tokens      │ │
│  │ MAX_RECORDS: 69           ││   │ MAX_RECORDS: 69           │ │
│  └───────────────────────────┘│   └───────────────────────────┘ │
│                               │                                 │
│  Seeds: [b"top_burn_shard",   │                                 │
│          index.to_le_bytes()] │                                 │
└─────────────────────────────────────────────────────────────────┘

# LATEST BURN SHARD

┌───────────────────────────────────────────────────────────────────┐
│                    LatestBurnShard (Circular Buffer)              │
├───────────────────────────────────────────────────────────────────┤
│                                                                   │
│  current_index: u8 = 25                                           │
│  records: Vec<BurnRecord> [MAX_RECORDS = 69]                      │
│                                                                   │
│    0    5    10   15   20   25   30   35   40   45   50   55   60 │
│  ┌───┬───┬───┬───┬───┬─►─┬───┬───┬───┬───┬───┬───┬───┬───┬───┐    │
│  │ 44│ 45│ 46│ 47│ 48│ 49│ 50│ 51│ 52│ 53│ 54│ 55│ 56│ 57│ 58│    │
│  └───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┘    │
│                         ▲                                         │
│                   Next write position                             │
│                                                                   │
│  When full, overwrites oldest records in circular fashion         │
│  Records 0-24: oldest burns                                       │
│  Records 25-68: newest burns                                      │
│                                                                   │
│  Seeds: [b"latest_burn_shard"]                                    │
└───────────────────────────────────────────────────────────────────┘

# USER PROFILE (MINT/BURN)

┌────────────────────────────────────────────────────────────────┐
│                   USER PROFILE (MINT/BURN)                     │
├────────────────────────────────────────────────────────────────┤
│                                                                │
│  ┌─────────────────────────────────────┐                       │
│  │           UserProfile               │                       │
│  │                                     │                       │
│  │  pubkey: Pubkey                     │                       │
│  │  total_minted: u64                  │                       │
│  │  total_burned: u64                  │                       │
│  │  mint_count: u64                    │                       │
│  │  burn_count: u64                    │                       │
│  │  created_at: i64                    │                       │
│  │  last_updated: i64                  │                       │
│  │  burn_history_index: Option<u64>    │ ──┐                   │
│  │                                     │   │                   │
│  │  Seeds: [b"user_profile", user_key] │   │                   │
│  └─────────────────────────────────────┘   │                   │
│                                            │                   │
│                                            │ Points to latest  │
│                                            ▼                   │
│  ┌─────────────────────────────────────┐                       │
│  │        UserBurnHistory #0           │ ◄── [FULL]            │
│  │                                     │                       │
│  │  owner: Pubkey                      │                       │
│  │  index: 0                           │                       │
│  │  signatures: Vec<String> [100/100]  │                       │
│  └─────────────────────────────────────┘                       │
│                                                                │
│  ┌─────────────────────────────────────┐                       │
│  │        UserBurnHistory #1           │ ◄── [CURRENT]         │
│  │                                     │                       │
│  │  owner: Pubkey                      │                       │
│  │  index: 1                           │                       │
│  │  signatures: Vec<String> [67/100]   │                       │
│  │                                     │                       │
│  │  Seeds: [b"burn_history",           │                       │
│  │          user_key,                  │                       │
│  │          index.to_le_bytes()]       │                       │
│  └─────────────────────────────────────┘                       │
│                                                                │
└────────────────────────────────────────────────────────────────┘