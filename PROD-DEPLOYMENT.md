## Keypair Management

### Setup Keypairs (First Time or New Environment)

```bash
# Setup testnet keypairs
./scripts/setup-keypairs.sh testnet

# Setup mainnet keypairs (in secure environment)
./scripts/setup-keypairs.sh mainnet
```

This script:
- Creates keypair directories in `~/.config/solana/memo-token/{env}/`
- Migrates existing keypairs (for testnet)
- Generates new keypairs (for mainnet)
- Sets proper permissions (700/600)

### Keypair Locations

After setup, all keypairs are stored in:
- Program keypairs: `~/.config/solana/memo-token/{env}/program-keypairs/`
- Authority keypairs: `~/.config/solana/memo-token/{env}/authority-keypairs/`

**Note:** These are outside the project directory and safe from build processes.