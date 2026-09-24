# LEZ Multisig Demo — CLI Runbook

Manual commands for a live demo. Run each block step-by-step.

## Setup

```bash
export LSSA_DIR=$HOME/lssa
export MULTISIG_DIR=$HOME/lez-multisig-framework
export REGISTRY_DIR=$HOME/lez-registry
export NSSA_WALLET_HOME_DIR=$MULTISIG_DIR/demo-wallet

WALLET="$LSSA_DIR/target/release/wallet"
MULTISIG="$MULTISIG_DIR/target/debug/multisig"
REGISTRY="$REGISTRY_DIR/target/debug/registry"
IDL="$MULTISIG_DIR/lez-multisig-ffi/src/multisig_idl.json"
TOKEN_BIN="$LSSA_DIR/artifacts/program_methods/token.bin"
REGISTRY_BIN="$REGISTRY_DIR/target/riscv32im-risc0-zkvm-elf/docker/registry.bin"
MULTISIG_BIN="$MULTISIG_DIR/target/riscv32im-risc0-zkvm-elf/docker/multisig.bin"
TOKEN_IDL="$MULTISIG_DIR/scripts/token-idl.json"
CREATE_KEY="demo-2026"
```

## 0. Reset sequencer (clean state)

```bash
pkill -f sequencer_runner || true; sleep 2
rm -rf $LSSA_DIR/sequencer_runner/rocksdb $LSSA_DIR/sequencer_runner/mempool
rm -f $NSSA_WALLET_HOME_DIR/storage.json
mkdir -p $NSSA_WALLET_HOME_DIR
cd $LSSA_DIR && nohup RUST_LOG=info ./target/release/sequencer_runner \
  ./sequencer_runner/configs/debug/ > /tmp/seq.log 2>&1 &
sleep 3 && curl -s http://127.0.0.1:3040/ > /dev/null && echo "Sequencer ready"

# Start mock Codex (for IDL upload)
nohup python3 $MULTISIG_DIR/scripts/mock-codex.py > /tmp/mock-codex.log 2>&1 &
```

## 1. Deploy programs

```bash
echo "demo" | $WALLET deploy-program $TOKEN_BIN
echo "demo" | $WALLET deploy-program $REGISTRY_BIN
echo "demo" | $WALLET deploy-program $MULTISIG_BIN
sleep 10  # wait for deploy txs to land
```

## 2. Capture program IDs

```bash
# Decimal CSV format — required for lez-cli --target-program-id
TOKEN_PROGRAM_ID=$($MULTISIG --idl $IDL inspect $TOKEN_BIN \
  | grep 'ProgramId (decimal)' | awk '{print $NF}')
REGISTRY_PROGRAM_ID=$($MULTISIG --idl $IDL inspect $REGISTRY_BIN \
  | grep 'ProgramId (decimal)' | awk '{print $NF}')
MULTISIG_PROGRAM_ID=$($MULTISIG --idl $IDL inspect $MULTISIG_BIN \
  | grep 'ProgramId (decimal)' | awk '{print $NF}')
export MULTISIG_PROGRAM_ID

# Hex format — required for registry CLI
TOKEN_PROGRAM_ID_HEX=$($MULTISIG --idl $IDL inspect $TOKEN_BIN \
  | grep 'ProgramId (hex)' | awk '{print $NF}' | tr -d ',')
REGISTRY_PROGRAM_ID_HEX=$($MULTISIG --idl $IDL inspect $REGISTRY_BIN \
  | grep 'ProgramId (hex)' | awk '{print $NF}' | tr -d ',')
MULTISIG_PROGRAM_ID_HEX=$($MULTISIG --idl $IDL inspect $MULTISIG_BIN \
  | grep 'ProgramId (hex)' | awk '{print $NF}' | tr -d ',')
```

## 3. Register programs in registry

```bash
# Create signer account first
$WALLET account new public --label signer
SIGNER="<base58 account_id from output>"

$REGISTRY register \
  --account $SIGNER \
  --registry-program $REGISTRY_PROGRAM_ID_HEX \
  --program-id $TOKEN_PROGRAM_ID_HEX \
  --name lez-token --version 0.1.0 \
  --description "Fungible token program for LEZ" \
  --idl-path $TOKEN_IDL \
  --tag token
sleep 10

$REGISTRY register \
  --account $SIGNER \
  --registry-program $REGISTRY_PROGRAM_ID_HEX \
  --program-id $MULTISIG_PROGRAM_ID_HEX \
  --name lez-multisig --version 0.1.0 \
  --description "M-of-N on-chain governance for LEZ" \
  --idl-path $IDL \
  --tag governance --tag multisig
sleep 10
```

## 4. List registry

```bash
$REGISTRY list --registry-program $REGISTRY_PROGRAM_ID_HEX
```

## 5. Create member accounts

```bash
$WALLET account new public --label m1
M1="<base58 account_id>"; M1_HEX="<hex from base58 decode>"

$WALLET account new public --label m2
M2="<base58 account_id>"; M2_HEX="<hex from base58 decode>"

$WALLET account new public --label m3
M3="<base58 account_id>"; M3_HEX="<hex from base58 decode>"

# Convert base58 account_id → hex (for --members arg):
python3 -c "
ALPHA='123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
s='<B58_ACCOUNT_ID>'
n=0
for c in s: n=n*58+ALPHA.index(c)
print(n.to_bytes(32,'big').hex())"
```

## 6. Create multisig (threshold=1, member=M1)

```bash
$MULTISIG --idl $IDL --program $MULTISIG_BIN \
  create-multisig \
    --create-key $CREATE_KEY \
    --threshold 1 \
    --members $M1_HEX \
    --member-accounts-account $M1

# Note "PDA multisig_state" from output:
MULTISIG_STATE="<pda from output>"
sleep 10
```

## 7. Propose adding M2 (proposal #0)

```bash
$WALLET account new public --label prop1
PROP1="<base58>"

$MULTISIG --idl $IDL --program $MULTISIG_BIN \
  propose-add-member \
    --new-member             $M2_HEX \
    --multisig-state-account $MULTISIG_STATE \
    --proposer-account       $M1 \
    --proposal-account       $PROP1 \
    --create-key             $CREATE_KEY \
    --proposal-index         0
sleep 10
```

## 8. Execute proposal #0 — M2 joins

```bash
$MULTISIG --idl $IDL --program $MULTISIG_BIN \
  execute \
    --proposal-index         0 \
    --multisig-state-account $MULTISIG_STATE \
    --executor-account       $M1 \
    --proposal-account       $PROP1 \
    --create-key             $CREATE_KEY
# M2 is now a member! Multisig is 1-of-2
sleep 10
```

## 9. Propose + execute adding M3 (proposal #1)

```bash
$WALLET account new public --label prop2
PROP2="<base58>"

$MULTISIG --idl $IDL --program $MULTISIG_BIN \
  propose-add-member \
    --new-member             $M3_HEX \
    --multisig-state-account $MULTISIG_STATE \
    --proposer-account       $M1 \
    --proposal-account       $PROP2 \
    --create-key             $CREATE_KEY \
    --proposal-index         1
sleep 10

$MULTISIG --idl $IDL --program $MULTISIG_BIN \
  execute \
    --proposal-index         1 \
    --multisig-state-account $MULTISIG_STATE \
    --executor-account       $M1 \
    --proposal-account       $PROP2 \
    --create-key             $CREATE_KEY
# M3 is now a member! Multisig is 1-of-3
sleep 10
```

## 9.5. Raise threshold to 2-of-3 (proposal #2)

```bash
$WALLET account new public --label prop-thresh
PROP_THRESH="<base58>"

$MULTISIG --idl $IDL --program $MULTISIG_BIN \
  propose-change-threshold \
    --new-threshold          2 \
    --multisig-state-account $MULTISIG_STATE \
    --proposer-account       $M1 \
    --proposal-account       $PROP_THRESH \
    --create-key             $CREATE_KEY \
    --proposal-index         2
sleep 10

$MULTISIG --idl $IDL --program $MULTISIG_BIN \
  execute \
    --proposal-index         2 \
    --multisig-state-account $MULTISIG_STATE \
    --executor-account       $M1 \
    --proposal-account       $PROP_THRESH \
    --create-key             $CREATE_KEY
# Multisig is now 2-of-3!
sleep 10
```

## 10. Token governance via ChainedCall (proposal #3)

### 10a. Create fungible token

```bash
$WALLET account new public --label token-def
TOKEN_DEF="<base58>"
$WALLET account new public --label token-holding
TOKEN_HOLDING="<base58>"

$WALLET token new \
  --definition-account-id "Public/$TOKEN_DEF" \
  --supply-account-id     "Public/$TOKEN_HOLDING" \
  --name LEZToken \
  --total-supply 1000000
sleep 10
```

### 10b. Compute vault PDA and fund it

```bash
# Compute vault seed + PDA (nssa_core LE formula — must match sequencer)
export CREATE_KEY MULTISIG_PROGRAM_ID
VAULT_COMPUTED=$(python3 - << 'PYEOF'
import hashlib, struct, os
ck = os.environ['CREATE_KEY'].encode()
tag = b'multisig_vault__'
tag_padded = tag + b'\x00' * (32 - len(tag))
seed = hashlib.sha256(tag_padded + ck.ljust(32, b'\x00')).digest()
PREFIX = b'/NSSA/v0.2/AccountId/PDA/\x00\x00\x00\x00\x00\x00\x00'
prog_id_u32 = [int(x) for x in os.environ['MULTISIG_PROGRAM_ID'].split(',')]
prog_id_bytes = b''.join(struct.pack('<I', x) for x in prog_id_u32)
buf = PREFIX + prog_id_bytes + seed
h = hashlib.sha256(buf).digest()
ALPHA = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
n = int.from_bytes(h, 'big')
b58 = ''
while n: b58 = ALPHA[n % 58] + b58; n //= 58
print(seed.hex(), b58)
PYEOF
)
read VAULT_SEED VAULT_PDA <<< "$VAULT_COMPUTED"
echo "Vault seed: $VAULT_SEED"
echo "Vault PDA:  $VAULT_PDA"

# Fund vault with 500 tokens
$WALLET token send \
  --from   "Public/$TOKEN_HOLDING" \
  --to     "Public/$VAULT_PDA" \
  --amount 500
sleep 10
```

### 10c. Create recipient account + propose token transfer

```bash
$WALLET account new public --label recipient
RECIPIENT="<base58>"
$WALLET account new public --label prop-token
PROP_TOKEN="<base58>"

# Transfer { amount_to_transfer: 200 } encoded as Vec<u32>:
# variant=0 (Transfer), u128(200) = 5 words: [0, 200, 0, 0, 0]
TARGET_IX_DATA="0,200,0,0,0"

# #40: bind the approved destinations INTO the proposal so the executor cannot
# substitute them. Same order the executor passes in --target-accounts-account
# below: [vault, recipient]. Encoding confirmed via `propose --help`:
#   --target-account-ids  Vec<[u8;32]>  — format: HEX64,...  (comma-separated, like --pda-seeds)
TARGET_IDS=$(python3 -c "import base58; print(','.join(base58.b58decode(a).hex() for a in ['$VAULT_PDA','$RECIPIENT']))")

$MULTISIG --idl $IDL --program $MULTISIG_BIN \
  propose \
    --target-program-id       $TOKEN_PROGRAM_ID \
    --target-instruction-data $TARGET_IX_DATA \
    --target-account-count    2 \
    --target-account-ids      $TARGET_IDS \
    --pda-seeds               $VAULT_SEED \
    --authorized-indices      0 \
    --multisig-state-account  $MULTISIG_STATE \
    --proposer-account        $M1 \
    --proposal-account        $PROP_TOKEN \
    --create-key              $CREATE_KEY \
    --proposal-index          3
sleep 10
```

### 10d. M2 approves (threshold=2, need one more vote)

```bash
$MULTISIG --idl $IDL --program $MULTISIG_BIN \
  approve \
    --proposal-index         3 \
    --multisig-state-account $MULTISIG_STATE \
    --approver-account       $M2 \
    --proposal-account       $PROP_TOKEN \
    --create-key             $CREATE_KEY
sleep 10
```

### 10e. Execute — ChainedCall transfers 200 LEZToken vault → recipient

```bash
$MULTISIG --idl $IDL --program $MULTISIG_BIN \
  execute \
    --proposal-index          3 \
    --multisig-state-account  $MULTISIG_STATE \
    --executor-account        $M1 \
    --proposal-account        $PROP_TOKEN \
    --create-key              $CREATE_KEY \
    --target-accounts-account "$VAULT_PDA,$RECIPIENT"
# 200 LEZToken transferred vault → recipient via ChainedCall!
sleep 10
```

## 11. Verify in registry

```bash
$REGISTRY info \
  --registry-program $REGISTRY_PROGRAM_ID_HEX \
  --program-id       $MULTISIG_PROGRAM_ID_HEX
```

## Tips

- Watch sequencer: `tail -f /tmp/seq.log`
- `--dry-run` on any command shows what would be submitted without sending
- If tx fails: check seq.log for `InvalidProgramBehavior` or `ProgramExecutionFailed`
- Rebuild wallet+sequencer together from same lssa rev if "Unknown program" appears
- `make generate-idl` after any source change to `multisig_program/src/lib.rs`
- ProgramId formats: lez-cli uses decimal CSV, registry CLI uses 64-char hex
- Vault PDA must be computed with Python LE formula (see 10b) — not `lez-cli pda`
- Rest accounts: comma-separated in single flag, e.g. `--target-accounts-account "addr1,addr2"`
