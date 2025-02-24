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
    getOrCreateAssociatedTokenAccount 
} from '@solana/spl-token';
import { expect } from 'chai';

describe("memo-token", () => {
    const provider = anchor.AnchorProvider.env();
    anchor.setProvider(provider);

    const program = anchor.workspace.MemoToken as Program<MemoToken>;
    
    // 这里需要替换为你的 mint 账户地址
    const mintAddress = new PublicKey("9AkxbAh31apMdjbG467VymHWJXtX92LCUz29nvtRYURS");
    let userTokenAccount: PublicKey;

    before(async () => {
        // 为测试用户创建代币账户
        const tokenAccount = await getOrCreateAssociatedTokenAccount(
            provider.connection,
            (provider.wallet as anchor.Wallet).payer,
            mintAddress,
            provider.wallet.publicKey
        );
        userTokenAccount = tokenAccount.address;
    });

    it("Processes transfer and mints token", async () => {
        // 发送交易
        const tx = await program.methods
            .processTransfer()
            .accounts({
                from: provider.wallet.publicKey,
                mint: mintAddress,
                mintAuthority: provider.wallet.publicKey,
                tokenAccount: userTokenAccount,
                tokenProgram: TOKEN_PROGRAM_ID,
            })
            .rpc();

        console.log("Transaction signature", tx);

        // 验证代币余额
        const balance = await provider.connection.getTokenAccountBalance(userTokenAccount);
        console.log("New token balance:", balance.value.uiAmount);
        expect(Number(balance.value.amount)).to.be.greaterThan(0);
    });
}); 