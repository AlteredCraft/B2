---
type: note
title: Encryption
---
The practice of transforming readable information so that only someone holding
the correct key can recover the original. It protects the confidentiality of
stored files and of traffic crossing an untrusted network.

## Symmetric and asymmetric

Two families do the work, and nearly every real system uses both. In the
symmetric family a single shared secret both scrambles and restores the data.
It is fast enough to run on a video stream or a whole disk, and it is what
actually protects the bulk of any conversation. Its weakness is arithmetic
rather than mathematical: for a group of people to talk privately in pairs, the
number of secrets to distribute grows with the square of the group, and there
is no obvious way to hand the first one over safely.

The asymmetric family solves exactly that. Each participant keeps a private
half and publishes a public half, and anything sealed with the public half can
only be opened with the private one. That makes it possible to send something
confidential to a stranger with no prior arrangement — but it is orders of
magnitude slower, so it is almost never used on the payload itself.

The practical arrangement is a hybrid. The slow public-key step runs once, at
the start, purely to agree on a fresh shared secret; the fast symmetric cipher
then carries the actual conversation. This is what happens invisibly in the
first few milliseconds of loading a web page, and it is why a browser can open
a private channel to a server it has never contacted before.

## Choosing a strength

Strength is quoted in bits, but the number means different things in the two
families and comparing them directly is a common mistake. Symmetric bits are
close to literal: an attacker must try up to two to the power of that many
combinations. Asymmetric bits describe the size of a number being factored or
a curve being searched, and the best known attacks are far better than brute
force, so far larger values are needed for equivalent safety.

The security level — the symmetric-equivalent strength — is the honest way to
compare, and it is what standards bodies actually publish:

| Security level | Symmetric | Finite-field / factoring | Elliptic curve | Status |
|---|---|---|---|---|
| 80 bits | 80 | 1024-bit | 160-bit | Broken; withdrawn |
| 112 bits | 112 | 2048-bit | 224-bit | Legacy only |
| 128 bits | 128 | 3072-bit | 256-bit | Current default |
| 192 bits | 192 | 7680-bit | 384-bit | High assurance |
| 256 bits | 256 | 15360-bit | 512-bit | Long-term archival |

The elliptic-curve column is why modern systems moved to curves: a 256-bit
curve buys the same protection as a number roughly twelve times longer, with
correspondingly smaller messages and faster arithmetic on a phone. Note also
how badly the factoring column scales — doubling the security level does not
double the key size, it multiplies it fivefold, which is why simply choosing
enormous classical keys was never a viable long-term answer.

Cipher modes are the other half of the choice and the half that is more often
got wrong. A raw block cipher applied to each block independently leaks
structure so plainly that a bitmap image encrypted that way is still visibly
recognizable. Modern practice is authenticated encryption, where the same
operation that conceals the data also produces a tag proving nobody altered it,
and a unique nonce per message is mandatory — repeating one under the same key
in a counter-based mode can hand an attacker the ability to forge messages,
not merely to read them.

## Keys in transit and at rest

An algorithm is rarely the weak part. Key management is: how the secret is
generated, where it lives, who can read it, and what happens when it leaks.
Random generation must come from the operating system's own entropy source,
because a predictable generator quietly reduces an enormous keyspace to a
handful of guessable values, and the resulting traffic looks perfectly normal
from the outside.

At rest, the usual answer is a hardware module or a platform keystore that
performs operations on request but never exports the private half, so a
compromise of the application does not become a permanent compromise of the
identity. Escrow and backup arrangements pull in the opposite direction and
have to be designed deliberately, since a copy kept for recovery is also a
copy an attacker can target.

## Forward secrecy

Suppose an adversary records every encrypted session today, stores it, and
years later obtains the server's long-term private key — by subpoena, theft, or
patience. If that key was what protected each session, the entire archive
decrypts at once, retroactively. Forward secrecy is the property that prevents
this: each session negotiates an ephemeral key pair used only for that
conversation and discarded immediately afterward, with the long-term key
reduced to proving identity rather than protecting content. Recovering it later
proves who the server was and reveals nothing about what was said.

The same reasoning drives the harvest-now-decrypt-later concern that motivates
post-quantum migration. Traffic captured today under conventional public-key
agreement stays vulnerable to a machine that does not exist yet, which is why
deployments have begun running a classical and a lattice-based exchange side by
side and mixing both results into the session secret, so that an attacker must
break both. That property is not a free consequence of running two exchanges —
it holds only when the combiner binds both results correctly, which is why the
hybrid constructions in deployment went through protocol review rather than
being assembled ad hoc.

## What it does not do

An authenticated mode does guard a message's integrity in transit, but what it
authenticates is possession of a key, never the human behind it — and nothing
here is anonymity. Scrambled
traffic still reveals its endpoints, its timing, and its volume, and those
patterns alone can identify a site being visited or a command being issued.
Nor does any of it help once the data is in use: a compromised endpoint sees
exactly what its user sees, which is why the practical attack on a private
conversation is almost never the mathematics and almost always the device, the
person, or the recovery path around them.
