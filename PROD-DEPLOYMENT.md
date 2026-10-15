
# MEMO Token Production Deployment Guide

This document describes how to deploy the MEMO Token project to the production environment (Mainnet).

## Architecture Overview

**Keypair layout (same on every machine; the network is part of the path):**

```
~/.config/solana/memo-token/
├── testnet/
│   ├── authority/   deploy_admin, memo_token_mint
│   └── programs/    memo_mint, memo_burn, memo_chat, memo_profile, memo_project, memo_blog, memo_forum
└── mainnet/
    ├── authority/   (same names)
    └── programs/    (same names; new programs are added here too)

Each keypair <name> is stored as:
  <name>-keypair.json.gpg   GPG-encrypted keypair (symmetric, AES256; one passphrase per network)
  <name>-keypair.pub        its public key (lets scripts verify configuration while locked)
  <name>-keypair.json       link to the decrypted file on a RAM disk, only while unlocked
```

`scripts/keys.sh` manages the keys:

```bash
./scripts/keys.sh unlock <testnet|mainnet>   # one passphrase; decrypts that network's keys into a RAM disk
./scripts/keys.sh lock                       # removes the links and decrypted keys, ejects the RAM disk
./scripts/keys.sh status                     # key states + any plaintext keypair files left on disk
./scripts/keys.sh import <net> <authority|programs> <file> [name]   # encrypt an existing keypair
./scripts/keys.sh new <net> <authority|programs> <name>             # generate one inside the RAM disk
./scripts/keys.sh migrate                    # encrypt keypairs from the old plaintext layout
```

The network whose keys are used is the **program_env**:
- Deploy scripts use the network they deploy. Prod Staging deploys the mainnet build, so it uses the **mainnet** keys.
- Admin clients use `Anchor.toml` `program_env` (the deploy scripts override it with `MEMO_PROGRAM_ENV`).

Program keypairs are needed only for a program's **first** deploy (they create the program at its address).
Deployed programs are upgraded by program ID with the upgrade authority (`deploy_admin`).

**Environment Config:**

```bash
export X1_RPC_URL="https://rpc.mainnet.x1.xyz"
```

### Keypair Management Strategy

**Key Principles:**
- ✅ **Encrypted at rest**: no plaintext keypair on disk; decrypted only into a RAM disk while unlocked
- ✅ **Per-network trees**: the path tells which network a key belongs to; testnet and mainnet use different passphrases
- ✅ **Same layout everywhere**: development and production machines use the same paths and scripts
- ✅ **Different Keypairs**: Mainnet uses completely different program IDs and authority keys
- ✅ **Offline backups**: keep the `.gpg` files and the passphrases in separate offline locations

---

## Prerequisites

### 1. Secure Production Machine Setup

**Requirements:**
- Dedicated, isolated production machine
- Never used for testing or development
- Hardened security configuration
- Access limited to authorized personnel only

### 2. Install All Dependencies

```bash
curl --proto '=https' --tlsv1.2 -sSfL https://raw.githubusercontent.com/solana-developers/solana-install/main/install.sh | bash
```

---

## Upgrading an Existing Deployment (Read First)

Mainnet programs are already live, so a release is normally an **upgrade**, not a first deploy.
Steps 2, 3 and 8 (new keypairs, new IDs, token creation) apply only to a brand-new deployment.

### Deployment Machine Setup

**Machine with keypairs in an old plaintext layout.** Older scripts kept authority keypairs in
`~/.config/solana/memo-token/authority/` and program keypairs in `programs/<env>/` or `<repo>/target/deploy/`.

1. Back up those keypair files to offline storage first.
2. Update the repository (`git pull`) to the release being deployed. Do not run `anchor clean` / `cargo clean`
   before the next step, because they delete `target/`.
3. Run `./scripts/keys.sh migrate` in your terminal. It identifies each keypair by its public key
   (code constants and `Anchor.toml`), asks for each network's passphrase (set on first use), encrypts the keys
   into the new layout, verifies every round trip, then offers to remove the plaintext files.
4. Run `./scripts/keys.sh status`. It lists any plaintext keypair files still on disk.

**New machine.** Copy `~/.config/solana/memo-token/<network>/` (the `.gpg` and `.pub` files) from the
offline backup, then `chmod 700` the directories and `chmod 600` the `.gpg` files.

**Every deployment session:**

```bash
./scripts/keys.sh unlock mainnet     # in your own terminal; asks for the mainnet passphrase
./scripts/deploy-prod-staging-testnet.sh    # or ./scripts/deploy-mainnet.sh
cargo run --bin verify-deployment -- <WALLET_WITH_MEMO>
./scripts/keys.sh lock
```

Notes:
- The deploy scripts check that `deploy_admin` matches `AUTHORIZED_ADMIN_PUBKEY` in the code and that it is
  the on-chain upgrade authority. If the keys are locked, they stop and tell you to unlock.
- Run `unlock` in your own terminal: the passphrase is read from the terminal and passed to GPG through a pipe.
  It never appears in arguments, files, or an AI assistant's session.
- `unlock` refuses to run if a plaintext keypair sits where a link goes.
- `Anchor.toml` `[provider] wallet` points at the testnet `deploy_admin` and is only the payer for test clients.
  Set `cluster` / `program_env` for the target network.

### Release Order: Client First, Then All Programs

Contract changes are not backward compatible with old clients:

- memo-mint requires the `mint_state` account (one mint per transaction).
- memo-burn requires the `global_burn_stats` account.
- Every chat / blog / forum / profile / project instruction that mints or burns passes these accounts as its last account.

Old clients and old programs do not pass these accounts, so their mint/burn calls fail against the new programs.
New clients also work against the old programs (Anchor ignores extra trailing accounts). So:

1. **Release the updated client (memo-app) first.** It works with both old and new programs.
2. **Upgrade all 7 programs in one run**: `./scripts/deploy-mainnet.sh` with no arguments.
   The script builds everything first, then upgrades in order:
   `memo_mint` → `memo_burn` → `memo_chat` → `memo_profile` → `memo_project` → `memo_blog` → `memo_forum`.
   Between the `memo_mint` / `memo_burn` upgrade and the upgrade of the dependent programs,
   mint/burn through those dependent programs fails. Do not stop halfway.
3. `mint_state` and `global_burn_stats` are created automatically by the deploy script right after
   `memo_mint` / `memo_burn` are deployed or upgraded. The init clients are idempotent.
   If that step fails, the script stops and prints the retry command. Until both accounts exist, every mint/burn fails.
4. Verify with `cargo run --bin verify-deployment -- <WALLET_WITH_MEMO>`. It is read-only: it runs simulations
   only and sends nothing. It uses `cluster` / `program_env` from `Anchor.toml` and checks:
   - all 7 programs: upgrade authority is `deploy_admin`, and the on-chain bytecode matches `target/deploy/*.so`
   - the MEMO mint authority is the memo-mint PDA
   - `mint_state` and `global_burn_stats` exist with the right owner, size and discriminator
   - simulated mints and burns:
     - a single mint succeeds
     - a second mint in the same transaction fails with `OneMintPerTransaction`
     - a 1 MEMO burn succeeds
     - old-client calls without the new accounts are rejected

   `<WALLET_WITH_MEMO>` is any address that holds MEMO and has burned before, needed for the mint and burn
   simulations; no private key is needed. Exit code 1 means a check failed.

   Then:
   - Prod Staging: also run the smoke tests (`./scripts/run-smoke-tests.sh`).
   - Mainnet: do one real mint and one small burn from the client. The smoke tests spend real MEMO there.

Always run the same release through **Prod Staging (Step 5)** first.

### On-chain IDL

For upgrades, the script runs `anchor idl upgrade` (falling back to `anchor idl init`) with retries.
If IDL sync still fails, the program upgrade has already succeeded. Rerun the printed command later.
This relies on Anchor 0.32's legacy IDL instructions. Anchor 1.x stores IDLs with the Program Metadata
program, which is not deployed on X1 yet. Stay on Anchor 0.32.x until it is.

---

## Step 1: Prepare Production Environment

### 1.1 Set Environment Variables

Add to `~/.bashrc`, `~/.profile` or `~/.zshrc` on production machine:

```bash
# X1 Network RPC (Mainnet)
export X1_RPC_URL="https://rpc.mainnet.x1.xyz"

# Set as default X1 cluster
export ANCHOR_PROVIDER_URL="$X1_RPC_URL"

# Reload configuration
source ~/.bashrc  # or source ~/.zshrc
```

### 1.2 Clone Repository

```bash
cd ~/
git clone https://github.com/xenartist/memo-token.git
cd memo-token
git checkout main  # or your production branch
```

---

## Step 2: Generate Mainnet Keypairs (ONLY FOR NEW KEYPAIRS)

### 2.1 Run Setup Script

```bash
./scripts/setup-keypairs.sh mainnet
```

For each missing keypair it offers: import from a backup file, generate a new one, or skip.
1. **Authority Keypairs**: `deploy_admin` and `memo_token_mint` in `~/.config/solana/memo-token/mainnet/authority/`
2. **Program Keypairs**: one per program in `~/.config/solana/memo-token/mainnet/programs/`

New keypairs are generated inside the RAM disk and stored encrypted; the plaintext never touches the disk.

### 2.2 Backup Keypairs Immediately

**🔐 CRITICAL**: Back up `~/.config/solana/memo-token/mainnet/` (the `.gpg` files) in multiple secure, offline
locations, and keep the passphrase separately.

---

## Step 3: Update Source Code with Mainnet IDs (ONLY FOR NEW KEYPAIRS)

### 3.1 Update Program IDs

After generating mainnet program keypairs, get their public keys:

```bash
cd ~/memo-token

# Get all program IDs
for program in memo_mint memo_burn memo_chat memo_profile memo_project memo_blog memo_forum; do
  echo "$program: $(cat ~/.config/solana/memo-token/mainnet/programs/${program}-keypair.pub)"
done
```

### 3.2 Update Contract Code

Edit each program's `src/lib.rs`:

```rust
// programs/memo-mint/src/lib.rs
#[cfg(feature = "mainnet")]
declare_id!("YOUR_MAINNET_PROGRAM_ID_HERE");  // ← Update this

#[cfg(not(feature = "mainnet"))]
declare_id!("A31a17bhgQyRQygeZa1SybytjbCdjMpu6oPr9M3iQWzy");  // Testnet (unchanged)
```

Repeat for all 5 programs: `memo-mint`, `memo-burn`, `memo-chat`, `memo-profile`, `memo-project`.

### 3.3 Update Authority Public Keys

Get authority public keys:

```bash
# Admin authority
cat ~/.config/solana/memo-token/mainnet/authority/deploy_admin-keypair.pub

# Mint authority
cat ~/.config/solana/memo-token/mainnet/authority/memo_token_mint-keypair.pub
```

Update in contract code:

```rust
// For memo-mint, memo-burn, memo-profile, memo-chat, memo-project
#[cfg(feature = "mainnet")]
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("YOUR_MAINNET_MINT_AUTHORITY");

#[cfg(not(feature = "mainnet"))]
pub const AUTHORIZED_MINT_PUBKEY: Pubkey = pubkey!("HLCoc7wNDavNMfWWw2Bwd7U7A24cesuhBSNkxZgvZm1");

// For memo-chat and memo-project only
#[cfg(feature = "mainnet")]
pub const AUTHORIZED_ADMIN_PUBKEY: Pubkey = pubkey!("YOUR_MAINNET_ADMIN_AUTHORITY");

#[cfg(not(feature = "mainnet"))]
pub const AUTHORIZED_ADMIN_PUBKEY: Pubkey = pubkey!("Gkxz6ogojD7Ni58N4SnJXy6xDxSvH5kPFCz92sTZWBVn");
```

### 3.4 Commit Changes

```bash
git add programs/*/src/lib.rs
git commit -m "feat: add mainnet program IDs and authority keys"
git push origin main
```

---

## Step 4: Fund Deployment Wallet

The deployment wallet (admin authority) needs XNT to pay for:
- Program deployment fees
- Account creation rent
- Transaction fees

```bash
# Check admin wallet address
cat ~/.config/solana/memo-token/mainnet/authority/deploy_admin-keypair.pub

# Fund this address with XNT on X1 Mainnet
# Recommended: At least 5-10 XNT for initial deployment

# Verify balance
solana balance $(cat ~/.config/solana/memo-token/mainnet/authority/deploy_admin-keypair.pub) \
  --url $X1_RPC_URL
```

---

## Step 5: Prod Staging Deploy to Testnet

### 5.1 Pre-Deployment Checklist

Before running deployment:

- [ ] All mainnet program IDs updated in code
- [ ] All authority public keys updated in code
- [ ] Changes committed to git
- [ ] Admin wallet funded with sufficient XNT
- [ ] All keypairs backed up to offline storage
- [ ] Running on secure, isolated production machine
- [ ] Contracts tested thoroughly on testnet

### 5.2 Run Deployment

```bash
cd ~/memo-token

# Deploy all programs
./scripts/deploy-prod-staging-testnet.sh

# Or deploy specific programs
./scripts/deploy-prod-staging-testnet.sh memo_burn memo_chat
```

The script will:
1. Verify the mainnet keys are unlocked and match the code
2. Verify program IDs and authorities match code
3. Build with `mainnet` feature flag
4. Deploy to X1 Testnet
5. Display deployment summary

### 5.3 Verify Deployment

After successful deployment, verify on block explorer:

```bash
# Check each program
for program in memo_mint memo_burn memo_chat memo_profile memo_project memo_blog memo_forum; do
  PROGRAM_ID=$(grep -A8 '^\[programs.mainnet\]' Anchor.toml | grep "^$program " | cut -d'"' -f2)
  echo "✅ $program: https://rpc.testnet.x1.xyz/address/$PROGRAM_ID"
done
```

---

## Step 6: Deploy to Mainnet

### 6.1 Pre-Deployment Checklist

Before running deployment:

- [ ] All mainnet program IDs updated in code
- [ ] All authority public keys updated in code
- [ ] Changes committed to git
- [ ] Admin wallet funded with sufficient XNT
- [ ] All keypairs backed up to offline storage
- [ ] Running on secure, isolated production machine
- [ ] Contracts tested thoroughly on testnet

### 6.2 Run Deployment

```bash
cd ~/memo-token

# Deploy all programs
./scripts/deploy-mainnet.sh

# Or deploy specific programs
./scripts/deploy-mainnet.sh memo_burn memo_chat
```

The script will:
1. Verify the mainnet keys are unlocked and match the code
2. Verify program IDs and authorities match code
3. Build with `mainnet` feature flag
4. Deploy to X1 Mainnet
5. Display deployment summary

### 6.3 Verify Deployment

After successful deployment, verify on block explorer:

```bash
# Check each program
for program in memo_mint memo_burn memo_chat memo_profile memo_project memo_blog memo_forum; do
  PROGRAM_ID=$(grep -A8 '^\[programs.mainnet\]' Anchor.toml | grep "^$program " | cut -d'"' -f2)
  echo "✅ $program: https://rpc.mainnet.x1.xyz/address/$PROGRAM_ID"
done
```

---

## Step 7: Initialize Global State Accounts

> `memo-mint`'s `mint_state` and `memo-burn`'s `global_burn_stats` are initialized automatically by the
> deploy script (Step 5 / Step 6). The accounts below are only needed on a **first** deployment.

### 7.1 Initialize Chat Global Counter

```bash
# Ensure X1_RPC_URL is set
echo $X1_RPC_URL  # Should output: https://rpc.mainnet.x1.xyz

# Run initialization
cd ~/memo-token
cargo run --bin admin-init-global-group-counter
```

### 7.2 Initialize Chat Burn Leaderboard

```bash
cargo run --bin admin-init-burn-leaderboard
```

### 7.3 Initialize Project Global Counter

```bash
cargo run --bin admin-memo-project-init-global-project-counter
```

### 7.4 Initialize Project Burn Leaderboard

```bash
cargo run --bin admin-memo-project-init-burn-leaderboard
```

**Note**: Admin tools use `cluster` / `program_env` from `Anchor.toml` and sign with that network's `deploy_admin`
(`./scripts/keys.sh unlock <network>` first).

---

## Step 8: Create MEMO Token

### 8.1 Prepare Token Metadata

Review and update if needed:
```bash
vim metadata/memo_token-metadata.json
```

### 8.2 Run Token Creation Script

```bash
cd ~/memo-token/clients/memo-token/src
chmod +x admin-create-memo-token.sh

# Run token creation
./admin-create-memo-token.sh
```

This will:
- Create the MEMO token with Token-2022 standard
- Set token metadata (name, symbol, URI)
- Configure decimals (6)
- Set mint authority to `memo_token_mint-keypair.json`
- Display the mint address

### 8.3 Record Mint Address

Save the output mint address - you'll need it for:
- Frontend configuration
- User documentation
- Future operations

### 8.4 Transfer Mint Authority to Program

After creating the token, transfer the mint authority to the memo-mint program's PDA for decentralized control:

```bash
cd ~/memo-token

# Build and run the authority transfer tool
cargo run --bin admin-transfer-memo-token-mint-authority
```

**What this does:**
- Calculates the PDA (Program Derived Address) for mint authority
- The PDA is derived from seeds: `["mint_authority"]` and the `memo_mint` program ID
- Transfers mint authority from `memo_token_mint-keypair.json` to this PDA
- Uses Token-2022 program for the authority transfer
- After this, only the memo-mint program can mint new tokens

---

## Step 9: Post-Deployment Security

### 9.1 Consider Transferring Program Upgrade Authority

For maximum security, consider transferring upgrade authority:

**Option 1: Set to Immutable** (No future upgrades)
```bash
for program in memo_mint memo_burn memo_chat memo_profile memo_project memo_blog memo_forum; do
  PROGRAM_ID=$(grep -A8 '^\[programs.mainnet\]' Anchor.toml | grep "^$program " | cut -d'"' -f2)
  solana program set-upgrade-authority $PROGRAM_ID --final --url $X1_RPC_URL
  echo "✅ $program set to immutable"
done
```

**Option 2: Transfer to Multisig** (Requires multiple signatures)
```bash
MULTISIG_ADDRESS="YOUR_MULTISIG_ADDRESS"

for program in memo_mint memo_burn memo_chat memo_profile memo_project memo_blog memo_forum; do
  PROGRAM_ID=$(grep -A8 '^\[programs.mainnet\]' Anchor.toml | grep "^$program " | cut -d'"' -f2)
  solana program set-upgrade-authority $PROGRAM_ID \
    --new-upgrade-authority $MULTISIG_ADDRESS \
    --url $X1_RPC_URL
  echo "✅ $program transferred to multisig"
done
```

### 9.2 Secure Keypair Storage

After deployment:

```bash
./scripts/keys.sh lock      # remove decrypted keys and links, eject the RAM disk
./scripts/keys.sh status    # every key "locked", no plaintext keypair files on disk
```

### 9.3 Backup Everything Again

```bash
# Final backup after deployment
# Store in secure offline location
```

---

## Environment Variables Reference

### Production Machine

```bash
# Add to ~/.bashrc or ~/.zshrc
export X1_RPC_URL="https://rpc.mainnet.x1.xyz"
export ANCHOR_PROVIDER_URL="$X1_RPC_URL"
```

### Test Machine

```bash
# Add to ~/.bashrc or ~/.zshrc
export X1_RPC_URL="https://rpc.testnet.x1.xyz"
export ANCHOR_PROVIDER_URL="$X1_RPC_URL"
```

**Important**: Client tools automatically read `X1_RPC_URL` from environment.

---

## Troubleshooting

### Deployment Script Verification Fails

If `deploy-mainnet.sh` reports mismatches:

1. **Check program IDs in code match keypairs** (only checked when the keypair is present):
   ```bash
   # Get actual ID from the stored keypair
   cat ~/.config/solana/memo-token/mainnet/programs/memo_mint-keypair.pub
   
   # Compare with code in programs/memo-mint/src/lib.rs
   grep -A 1 'cfg(feature = "mainnet")' programs/memo-mint/src/lib.rs
   ```

2. **Update code if needed** and recommit

3. **Re-run deployment**

### Admin Tools: "Failed to read admin keypair" / "not found"

The keys are locked, or the tool targets another network than you expect:
```bash
./scripts/keys.sh status            # which networks are unlocked
grep program_env Anchor.toml        # the network admin tools use (MEMO_PROGRAM_ENV overrides it)
./scripts/keys.sh unlock mainnet    # in your terminal
```

### Admin Tools: "UnauthorizedAdmin" Error

The admin keypair public key must match `AUTHORIZED_ADMIN_PUBKEY` in contract code:

```bash
# Get your admin public key
cat ~/.config/solana/memo-token/mainnet/authority/deploy_admin-keypair.pub

# Compare with code
grep "AUTHORIZED_ADMIN_PUBKEY" programs/memo-chat/src/lib.rs
```

---

## Security Best Practices

### ✅ DO

- Use dedicated, isolated production machine
- Keep every keypair encrypted (`scripts/keys.sh`); lock the keys as soon as a session ends
- Create multiple encrypted backups in offline storage
- Test thoroughly on testnet before mainnet deployment
- Consider multisig or governance for upgrade authority
- Monitor program accounts regularly
- Keep deployment logs

### ❌ DON'T

- Never use the same machine for test and production
- Never commit keypairs to git
- Never share keypairs via insecure channels
- Never run production deployments from development machines
- Never skip backup verification
- Never deploy without thorough testing

---

## Support & Resources

- **X1 Network Docs**: https://docs.x1.xyz

---

## Deployment Checklist

Use this checklist for each mainnet deployment:

- [ ] All prerequisites installed on production server
- [ ] Environment variables set (`X1_RPC_URL`)
- [ ] Repository cloned and on correct branch
- [ ] Mainnet keypairs stored encrypted (`setup-keypairs.sh` / `keys.sh migrate`); `keys.sh status` shows no plaintext files
- [ ] All keypairs backed up to offline storage
- [ ] Program IDs updated in contract code (all 7 programs)
- [ ] Authority public keys updated in contract code
- [ ] Changes committed to git
- [ ] Admin wallet funded with sufficient XNT
- [ ] Updated client (memo-app) released before the program upgrade
- [ ] Prod Staging (testnet) deployment verified
- [ ] Deployment script executed successfully
- [ ] `mint_state` and `global_burn_stats` initialized (deploy script output)
- [ ] `verify-deployment` passes with no failures
- [ ] `./scripts/keys.sh lock` run; `status` shows every key locked
- [ ] One real mint and one burn from the client succeed
- [ ] All programs verified on block explorer
- [ ] Global state accounts initialized
- [ ] MEMO token created
- [ ] Post-deployment security measures applied
- [ ] Final backup created and stored securely
- [ ] Deployment documented (date, versions, addresses)
