# Goal: a clearing network of two-party accounts

A statement of direction, written 24 November 2026. It says where the design is meant to end, not what the scripts
do today; what exists is in `DESIGN-COMMITTEE.md` (sections 3b and 8a).

## What a user should get

1. **One account, one risk number.** Everything the user holds (spot, futures, options) is netted, and margin is asked
   for the net risk only.
2. **Any counterparty.** Entering and leaving a position does not depend on who is on the other side.
3. **One price.** Every market and every application reads the same signed price.
4. **Gains are usable.** A gain of one position is margin for another, or can be taken out, without closing.
5. **Default is handled in steps.** An account in trouble is reduced part by part; it is not forfeited whole.

These are the services of a clearing house. The goal is to give them with no clearing house: no operator, no object
that every user touches.

## The form on Kaspa

A network of accounts, each between two parties. A few clearing parties (executors) stand in the middle; anyone can
be one.

| Service | Form |
|---|---|
| One account, one risk number | one UTXO between two parties: several legs, one margin, closed and liquidated on the whole |
| Any counterparty | a side is handed over without the party that stays; a clearing party takes the user's side at once |
| A leg moves between accounts | one transaction takes it out of one account and puts it into another, or does nothing |
| One price | the committee's signed price, read by spot, futures and options alike |
| Gains are usable | the entry is moved to a signed price, so that the gain becomes margin |
| Spot as margin | a token held in the account counts at a discount; a spot purchase and a futures sale in one transaction |
| Default in steps | legs are handed to a liquidator one at a time, at a discount |
| One bond behind many accounts | a clearing party's bond that many accounts refer to, and touch only at its default |

## The hard part

A clearing house posts no margin; between two parties both sides do. A clearing party's capital is therefore split
over its accounts, and this is the whole difference in capital efficiency. **One bond behind many accounts** is the
piece that closes it: with it a clearing party faces many users on little capital. It is the first thing to research.

## Order of work

1. One bond behind many accounts.
2. Gains made usable; liquidation in steps.
3. More legs in an account.
4. Spot tokens as margin.
5. Options.

Each is an extension of the position as it is; none replaces it.

## Not goals

- A funding rate computed from the total open interest.
- An insurance fund shared by everyone.
- One pool that every trade touches.
- An operator, an admin key, or a party that must be trusted to stay.
