import { PublicKey } from '@solana/web3.js';
import bs58 from 'bs58';
import { MangoClient } from '../../client';
import { I80F48, I80F48Dto } from './I80F48';

export class Bank {
  public depositIndex: I80F48;
  public borrowIndex: I80F48;

  static from(
    publicKey: PublicKey,
    obj: {
      group: PublicKey;
      mint: PublicKey;
      vault: PublicKey;
      oracle: PublicKey;
      depositIndex: I80F48Dto;
      borrowIndex: I80F48Dto;
      indexedTotalDeposits: I80F48Dto;
      indexedTotalBorrows: I80F48Dto;
      maintAssetWeight: I80F48Dto;
      initAssetWeight: I80F48Dto;
      maintLiabWeight: I80F48Dto;
      initLiabWeight: I80F48Dto;
      liquidationFee: I80F48Dto;
      dust: Object;
      tokenIndex: number;
    },
  ) {
    return new Bank(
      publicKey,
      obj.group,
      obj.mint,
      obj.vault,
      obj.oracle,
      obj.depositIndex,
      obj.borrowIndex,
      obj.indexedTotalDeposits,
      obj.indexedTotalBorrows,
      obj.maintAssetWeight,
      obj.initAssetWeight,
      obj.maintLiabWeight,
      obj.initLiabWeight,
      obj.liquidationFee,
      obj.dust,
      obj.tokenIndex,
    );
  }

  constructor(
    public publicKey: PublicKey,
    public group: PublicKey,
    public mint: PublicKey,
    public vault: PublicKey,
    public oracle: PublicKey,
    depositIndex: I80F48Dto,
    borrowIndex: I80F48Dto,
    indexedTotalDeposits: I80F48Dto,
    indexedTotalBorrows: I80F48Dto,
    maintAssetWeight: I80F48Dto,
    initAssetWeight: I80F48Dto,
    maintLiabWeight: I80F48Dto,
    initLiabWeight: I80F48Dto,
    liquidationFee: I80F48Dto,
    dust: Object,
    public tokenIndex: number,
  ) {
    this.depositIndex = I80F48.from(depositIndex);
    this.borrowIndex = I80F48.from(borrowIndex);
  }

  toString(): string {
    return `Bank ${
      this.tokenIndex
    } deposit index - ${this.depositIndex.toNumber()}, borrow index - ${this.borrowIndex.toNumber()}`;
  }
}

export class MintInfo {
  static from(
    publicKey: PublicKey,
    obj: {
      mint: PublicKey;
      bank: PublicKey;
      vault: PublicKey;
      oracle: PublicKey;
      addressLookupTable: PublicKey;
      tokenIndex: Number;
      addressLookupTableBankIndex: Number;
      addressLookupTableOracleIndex: Number;
      reserved: unknown;
    },
  ) {
    return new MintInfo(publicKey, obj.mint, obj.bank, obj.vault, obj.oracle);
  }

  constructor(
    public publicKey: PublicKey,
    public mint: PublicKey,
    public bank: PublicKey,
    public vault: PublicKey,
    public oracle: PublicKey,
  ) {}
}

export async function getMintInfoForTokenIndex(
  client: MangoClient,
  groupPk: PublicKey,
  tokenIndex: number,
): Promise<MintInfo[]> {
  const tokenIndexBuf = Buffer.alloc(2);
  tokenIndexBuf.writeUInt16LE(tokenIndex);
  return (
    await client.program.account.mintInfo.all([
      {
        memcmp: {
          bytes: groupPk.toBase58(),
          offset: 8,
        },
      },
      {
        memcmp: {
          bytes: bs58.encode(tokenIndexBuf),
          offset: 200,
        },
      },
    ])
  ).map((tuple) => {
    return MintInfo.from(tuple.publicKey, tuple.account);
  });
}
