
# MEMO Token Production Deployment Guide

This document describes how to deploy the MEMO Token project to the production environment (Mainnet).

## Architecture Overview

**Project Architecture (Based on Physical Environment Isolation):**

Test Server Production Server
├── target/deploy/
│ ├── memo_mint-keypair.json
│ ├── memo_burn-keypair.json
│ ├── memo_chat-keypair.json
│ ├── memo_profile-keypair.json
│ └── memo_project-keypair.json
│ 
└── ~/.config/solana/memo-token/authority/
│ ├── deploy_admin-keypair.json
│ └── memo_token_mint-keypair.json

**Environment Config: Environment Config:**

```bash
export X1_RPC_URL= "https://rpc.mainnet.x1.xyz"
```

or

# edit ~/.bashrc or ~/.zshrc on production machine, and reload
export X1_RPC_URL="https://rpc.mainnet.x1.xyz"

### Keypair Management Strategy


**Key Principles:**
- ✅ **Physical Isolation**: Test and production use completely separate machines
- ✅ **Unified Paths**: Same directory structure on both environments
- ✅ **Different Keypairs**: Mainnet uses completely different program IDs and authority keys
- ✅ **Environment Variables**: Network switching via `X1_RPC_URL` environment variable

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

## Step 1: Prepare Production Environment

### 1.1 Set Environment Variables

Add to `~/.bashrc` or `~/.zshrc` on production server:

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

## Step 2: Generate Mainnet Keypairs

### 2.1 Run Setup Script

```bash
./scripts/setup-keypairs.sh mainnet
```

This will guide you through:
1. **Program Keypairs**: Generate new keypairs in `target/deploy/`
2. **Authority Keypairs**: Generate admin and mint authority in `~/.config/solana/memo-token/authority/`

### 2.2 Backup Keypairs Immediately

**🔐 CRITICAL**: Backup keypairs in multiple secure, offline locations.

---

## Step 3: Update Source Code with Mainnet IDs

### 3.1 Update Program IDs

After generating mainnet program keypairs, get their public keys:

```bash
cd ~/memo-token

# Get all program IDs
for program in memo_mint memo_burn memo_chat memo_profile memo_project; do
  echo "$program: $(solana-keygen pubkey target/deploy/${program}-keypair.json)"
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
solana-keygen pubkey ~/.config/solana/memo-token/authority/deploy_admin-keypair.json

# Mint authority
solana-keygen pubkey ~/.config/solana/memo-token/authority/memo_token_mint-keypair.json
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
solana-keygen pubkey ~/.config/solana/memo-token/authority/deploy_admin-keypair.json

# Fund this address with XNT on X1 Mainnet
# Recommended: At least 5-10 XNT for initial deployment

# Verify balance
solana balance $(solana-keygen pubkey ~/.config/solana/memo-token/authority/deploy_admin-keypair.json) \
  --url $X1_RPC_URL
```

---

## Step 5: Deploy to Mainnet

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
./scripts/deploy-mainnet.sh

# Or deploy specific programs
./scripts/deploy-mainnet.sh memo_burn memo_chat
```

The script will:
1. Verify all keypairs exist
2. Verify program IDs and authorities match code
3. Build with `mainnet` feature flag
4. Deploy to X1 Mainnet
5. Display deployment summary

### 5.3 Verify Deployment

After successful deployment, verify on block explorer:

```bash
# Check each program
for program in memo_mint memo_burn memo_chat memo_profile memo_project; do
  PROGRAM_ID=$(solana-keygen pubkey target/deploy/${program}-keypair.json)
  echo "✅ $program: https://explorer.mainnet.x1.xyz/address/$PROGRAM_ID"
done
```

---

## Step 6: Initialize Global State Accounts

### 6.1 Initialize Chat Global Counter

```bash
# Ensure X1_RPC_URL is set
echo $X1_RPC_URL  # Should output: https://rpc.mainnet.x1.xyz

# Run initialization
cd ~/memo-token
cargo run --bin admin-init-global-group-counter
```

### 6.2 Initialize Chat Burn Leaderboard

```bash
cargo run --bin admin-init-burn-leaderboard
```

### 6.3 Initialize Project Global Counter

```bash
cargo run --bin admin-memo-project-init-global-project-counter
```

### 6.4 Initialize Project Burn Leaderboard

```bash
cargo run --bin admin-memo-project-init-burn-leaderboard
```

**Note**: All admin tools automatically use `X1_RPC_URL` from environment and `deploy_admin-keypair.json` for authorization.

---

## Step 7: Create MEMO Token

### 7.1 Prepare Token Metadata

Review and update if needed:
```bash
vim metadata/memo_token-metadata.json
```

### 7.2 Run Token Creation Script

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

### 7.3 Record Mint Address

Save the output mint address - you'll need it for:
- Frontend configuration
- User documentation
- Future operations

---

## Step 8: Post-Deployment Security

### 8.1 Consider Transferring Program Upgrade Authority

For maximum security, consider transferring upgrade authority:

**Option 1: Set to Immutable** (No future upgrades)
```bash
for program in memo_mint memo_burn memo_chat memo_profile memo_project; do
  PROGRAM_ID=$(solana-keygen pubkey target/deploy/${program}-keypair.json)
  solana program set-upgrade-authority $PROGRAM_ID --final --url $X1_RPC_URL
  echo "✅ $program set to immutable"
done
```

**Option 2: Transfer to Multisig** (Requires multiple signatures)
```bash
MULTISIG_ADDRESS="YOUR_MULTISIG_ADDRESS"

for program in memo_mint memo_burn memo_chat memo_profile memo_project; do
  PROGRAM_ID=$(solana-keygen pubkey target/deploy/${program}-keypair.json)
  solana program set-upgrade-authority $PROGRAM_ID \
    --new-upgrade-authority $MULTISIG_ADDRESS \
    --url $X1_RPC_URL
  echo "✅ $program transferred to multisig"
done
```

### 8.2 Secure Keypair Storage

After deployment:

```bash
# Option 1: Keep program keypairs in target/deploy/ (needed if programs are upgradeable)
# Ensure proper file permissions
chmod 600 ~/memo-token/target/deploy/*-keypair.json

# Option 2: Move to more secure location
mkdir -p ~/.config/solana/memo-token/program-keypairs
mv ~/memo-token/target/deploy/*-keypair.json \
   ~/.config/solana/memo-token/program-keypairs/

# Authority keypairs should remain in ~/.config/solana/memo-token/authority/
chmod 700 ~/.config/solana/memo-token/authority
chmod 600 ~/.config/solana/memo-token/authority/*
```

### 8.3 Backup Everything Again

```bash
# Final backup after deployment
# Store in secure offline location
```

---

## Environment Variables Reference

### Production Server

```bash
# Add to ~/.bashrc or ~/.zshrc
export X1_RPC_URL="https://rpc.mainnet.x1.xyz"
export ANCHOR_PROVIDER_URL="$X1_RPC_URL"
```

### Test Server

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

1. **Check program IDs in code match keypairs**:
   ```bash
   # Get actual ID from keypair
   solana-keygen pubkey target/deploy/memo_mint-keypair.json
   
   # Compare with code in programs/memo-mint/src/lib.rs
   grep -A 1 'cfg(feature = "mainnet")' programs/memo-mint/src/lib.rs
   ```

2. **Update code if needed** and recommit

3. **Re-run deployment**

### Admin Tools: "Failed to read admin keypair"

Ensure:
```bash
# Check file exists
ls -la ~/.config/solana/memo-token/authority/deploy_admin-keypair.json

# Check permissions
chmod 600 ~/.config/solana/memo-token/authority/deploy_admin-keypair.json
```

### Admin Tools: "UnauthorizedAdmin" Error

The admin keypair public key must match `AUTHORIZED_ADMIN_PUBKEY` in contract code:

```bash
# Get your admin public key
solana-keygen pubkey ~/.config/solana/memo-token/authority/deploy_admin-keypair.json

# Compare with code
grep "AUTHORIZED_ADMIN_PUBKEY" programs/memo-chat/src/lib.rs
```

---

## Security Best Practices

### ✅ DO

- Use dedicated, isolated production machine
- Set file permissions to 600 for all keypairs
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
- [ ] Mainnet keypairs generated via `setup-keypairs.sh`
- [ ] All keypairs backed up to offline storage
- [ ] Program IDs updated in contract code (all 5 programs)
- [ ] Authority public keys updated in contract code
- [ ] Changes committed to git
- [ ] Admin wallet funded with sufficient SOL
- [ ] Deployment script executed successfully
- [ ] All programs verified on block explorer
- [ ] Global state accounts initialized
- [ ] MEMO token created
- [ ] Post-deployment security measures applied
- [ ] Final backup created and stored securely
- [ ] Deployment documented (date, versions, addresses)
