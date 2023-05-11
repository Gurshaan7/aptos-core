import { AptosAccount, AptosClient, BCS, HexString, TxnBuilderTypes } from "../../../dist";
import { Timer } from "timer-node";
import { fetch } from "fetch-h2";
import { exit } from "process";

// const FULLNODE_URL = "http://0.0.0.0:8080/v1";
// const FAUCET_URL = "http://0.0.0.0:8081";

const FULLNODE_URL = "https://fullnode.testnet.aptoslabs.com/v1";
const FAUCET_URL = "https://faucet.testnet.aptoslabs.com";

async function main() {
  const timer = new Timer();

  const accountsCount = 5; // creates 400 accounts in total
  const firstPass = 100;
  const readAmplification = 100; // tests 200 accounts * 3 = 600 get calls
  let accountSequenceNumber: AccountSequenceNumbers | null = null;

  console.log("starting...");
  timer.start();
  // create accounts
  const accounts: AptosAccount[] = [];
  const recipients: AptosAccount[] = [];
  for (let i = 0; i < accountsCount; i++) {
    accounts.push(new AptosAccount());
    recipients.push(new AptosAccount());
  }
  console.log(`${accounts.length * 2} accounts created`);
  console.log(timer.time());

  // funds accounts
  const funds: string[] = [];

  for (let i = 0; i < accounts.length; i++) {
    funds.push(`${FAUCET_URL}/mint?address=${HexString.ensure(accounts[i].address()).noPrefix()}&amount=${100000000}`);
  }
  for (let i = 0; i < recipients.length; i++) {
    funds.push(`${FAUCET_URL}/mint?address=${HexString.ensure(recipients[i].address()).noPrefix()}&amount=${0}`);
  }
  // send requests
  const responses = await Promise.all(
    funds.map((fund) =>
      fetch(fund, {
        method: "POST",
        headers: {
          Authorization: `Bearer ejfklsdfj7vr4388fhhfh3f78hf345345hf00da0`,
        },
      }),
    ),
  );

  // read bodies
  await Promise.all(responses.map((resp) => resp.json()));

  // sleeps to let faucet do its work without the need to implement
  // waitForTransaction in this new client
  await sleep(15000); // 15 seconds
  console.log(`${funds.length} accounts funded`);
  console.log(timer.time());

  // read accounts
  const balances: string[] = [];
  for (let j = 0; j < readAmplification; j++) {
    for (let i = 0; i < accounts.length; i++) {
      balances.push(`${FULLNODE_URL}/accounts/${accounts[i].address().hex()}`);
    }
  }
  // send requests
  const balancesresponses = await Promise.all(balances.map((balance) => fetch(balance)));
  // read bodies
  await Promise.all(balancesresponses.map((resp) => resp.json()));

  //await Promise.all(balances);
  console.log(`${balances.length} balances checked`);
  console.log(timer.time());

  // initialize accounts with sequence number

  // array to hold the sequence number class initialization of an account
  const accountSequenceNumbers: AccountSequenceNumbers[] = [];
  // array to hold prmoises to fetch account current sequence number
  const awaitSequenceNumbers: Promise<void>[] = [];
  for (let i = 0; i < accounts.length; i++) {
    accountSequenceNumber = new AccountSequenceNumbers(accounts[i]);
    awaitSequenceNumbers.push(accountSequenceNumber.initialize());
    accountSequenceNumbers.push(accountSequenceNumber);
  }

  await Promise.all(awaitSequenceNumbers);
  console.log(`${accounts.length} accounts initialized`);
  console.log(timer.time());

  // submit transactions
  let bcsTxns: Uint8Array[] = [];
  for (let i = 0; i < firstPass; i++) {
    for (let j = 0; j < accountsCount; j++) {
      let sender = accounts[j];
      let recipient = recipients[j].address().hex();
      let sequenceNumber: bigint = await accountSequenceNumbers[j].nextSequenceNumber();
      let bcsTxn = transafer(sender, recipient, sequenceNumber, 1);
      bcsTxns.push(bcsTxn);
    }
  }

  // send requests
  const bcsTxnsresponses = await Promise.all(
    bcsTxns.map((bcsTxn) =>
      fetch(`${FULLNODE_URL}/transactions`, {
        method: "POST",
        body: Buffer.from(bcsTxn),
        headers: {
          "content-type": "application/x.aptos.signed_transaction+bcs",
        },
      }),
    ),
  );
  // read bodies
  const transactions = await Promise.all(bcsTxnsresponses.map((resp) => resp.json()));
  console.log(`${bcsTxns.length} transaction submitted`);
  console.log(timer.time());

  // check for transactions
  const waitFor: Promise<void>[] = [];
  for (let i = 0; i < transactions.length; i++) {
    waitFor.push(accountSequenceNumber!.synchronize());
  }

  await Promise.all(waitFor);
  console.log("transactions commited");
  console.log(timer.time());

  exit(0);
}

function transafer(sender: AptosAccount, recipient: string, sequenceNumber: bigint, amount: number): Uint8Array {
  const token = new TxnBuilderTypes.TypeTagStruct(TxnBuilderTypes.StructTag.fromString("0x1::aptos_coin::AptosCoin"));

  const entryFunctionPayload = new TxnBuilderTypes.TransactionPayloadEntryFunction(
    TxnBuilderTypes.EntryFunction.natural(
      "0x1::coin",
      "transfer",
      [token],
      [BCS.bcsToBytes(TxnBuilderTypes.AccountAddress.fromHex(recipient)), BCS.bcsSerializeUint64(amount)],
    ),
  );

  const rawTransaction = new TxnBuilderTypes.RawTransaction(
    // Transaction sender account address
    TxnBuilderTypes.AccountAddress.fromHex(sender.address()),
    BigInt(sequenceNumber),
    entryFunctionPayload,
    // Max gas unit to spend
    BigInt(200000),
    // Gas price per unit
    BigInt(100),
    // Expiration timestamp. Transaction is discarded if it is not executed within 20 seconds from now.
    BigInt(Math.floor(Date.now() / 1000) + 20),
    new TxnBuilderTypes.ChainId(2),
  );

  const bcsTxn = AptosClient.generateBCSTransaction(sender, rawTransaction);
  return bcsTxn;
  // const txn = await submitTransaction(bcsTxn);
  // return txn.hash;
}

async function get(path: string): Promise<any> {
  const response = await fetch(`${FULLNODE_URL}/${path}`, {
    headers: {
      "Content-Type": "application/json",
    },
  });
  const res = await response.json();
  return res;
}

async function sleep(ms: number): Promise<void> {
  return new Promise<void>((resolve) => setTimeout(resolve, ms));
}

class AccountSequenceNumbers {
  account: AptosAccount;
  lastUncommintedNumber: BCS.Uint64 | null = null;
  currentNumber: BCS.Uint64 | null = null;
  lock = false;
  maximumInFlight = 50;
  sleepTime = 10;
  maxWaitTime = 30; // in seconds

  constructor(acccount: AptosAccount) {
    this.account = acccount;
  }

  async initialize(): Promise<void> {
    const data = await get(`accounts/${this.account.address().hex()}`);

    const response = Promise.all(`accounts/${this.account.address().hex()}`);
    this.currentNumber = BigInt(data.sequence_number);
    this.lastUncommintedNumber = BigInt(data.sequence_number);
  }

  async update() {
    const { sequence_number } = await get(`accounts/${this.account.address().hex()}`);
    this.lastUncommintedNumber = BigInt(sequence_number);
    return this.lastUncommintedNumber;
  }

  async nextSequenceNumber(): Promise<bigint> {
    /*
    `lock` is used to prevent multiple coroutines from accessing a shared resource at the same time, which can result in race conditions and data inconsistency.
    This implementation is not as robust as using a proper lock implementation 
    like `async-mutex` because it relies on busy waiting to acquire the lock, 
    which can be less efficient and may not work well in all scenarios
    */
    while (this.lock) {
      await sleep(this.sleepTime);
    }

    this.lock = true;
    let nextNumber = BigInt(0);
    try {
      if (this.lastUncommintedNumber === null || this.currentNumber === null) {
        await this.initialize();
      }

      if (this.currentNumber! - this.lastUncommintedNumber! >= this.maximumInFlight) {
        await this.update();

        const startTime = Math.floor(Date.now() / 1000);
        while (this.lastUncommintedNumber! - this.currentNumber! >= this.maximumInFlight) {
          await sleep(this.sleepTime);
          if (Math.floor(Date.now() / 1000) - startTime > this.maxWaitTime) {
            console.warn(`Waited over 30 seconds for a transaction to commit, resyncing ${this.account.address()}`);
            await this.initialize();
          } else {
            await this.update();
          }
        }
      }
      nextNumber = this.currentNumber!;
      this.currentNumber!++;
    } catch (e) {
      console.error("error", e);
    } finally {
      this.lock = false;
    }
    return nextNumber;
  }

  async synchronize() {
    if (this.lastUncommintedNumber == this.currentNumber) return;

    await this.update();
    const startTime = Math.floor(Date.now() / 1000);
    while (this.lastUncommintedNumber != this.currentNumber) {
      if (Math.floor(Date.now() / 1000) - startTime > this.maxWaitTime) {
        console.warn(`Waited over 30 seconds for a transaction to commit, resyncing ${this.account.address()}`);
        await this.initialize();
      } else {
        await sleep(this.sleepTime);
        await this.update();
      }
    }
  }
}

main();
