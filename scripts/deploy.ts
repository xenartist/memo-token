import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { MemoToken } from "../target/types/memo_token";
import { 
    PublicKey, 
    SystemProgram, 
    SYSVAR_RENT_PUBKEY,
    Keypair 
} from '@solana/web3.js';
import { 
    TOKEN_PROGRAM_ID, 
    createMint,
    getOrCreateAssociatedTokenAccount,
    MINT_SIZE,
    createInitializeMintInstruction,
} from '@solana/spl-token';

async function main() {
    // 配置客户端
    const provider = anchor.AnchorProvider.env();
    anchor.setProvider(provider);

    const program = anchor.workspace.MemoToken as Program<MemoToken>;

    // 获取 mint PDA
    const [mintPda, mintBump] = PublicKey.findProgramAddressSync(
        [Buffer.from("memo_mint")],
        program.programId
    );

    console.log("Program ID:", program.programId.toString());
    console.log("Mint PDA:", mintPda.toString());

    try {
        // 创建一个新的 mint 账户
        const mint = await createMint(
            provider.connection,
            (provider.wallet as anchor.Wallet).payer,
            provider.wallet.publicKey,  // mint authority
            provider.wallet.publicKey,  // freeze authority
            9                          // decimals
        );

        console.log("Mint account created:", mint.toBase58());

        // 为当前钱包创建代币账户
        const tokenAccount = await getOrCreateAssociatedTokenAccount(
            provider.connection,
            (provider.wallet as anchor.Wallet).payer,
            mint,
            provider.wallet.publicKey
        );

        console.log("Current wallet token account:", tokenAccount.address.toString());

        // 打印重要信息
        console.log("\nImportant addresses:");
        console.log("Program ID:", program.programId.toString());
        console.log("Mint account:", mint.toBase58());
        console.log("Your wallet:", provider.wallet.publicKey.toString());
        console.log("Your token account:", tokenAccount.address.toString());

    } catch (error) {
        console.error("Error:", error);
    }
}

main().then(
    () => process.exit(0),
).catch(
    (error) => {
        console.error(error);
        process.exit(1);
    }
); 