# Future work: LE legacy out-of-band pairing in the Linux kernel

Status: **not started.** This is the one piece of seamless pairing that Linux cannot do today, and
the only difference left between pairing on Linux and pairing on Windows (findings/008).

## Why it matters

Seamless pairing hands the pen a 128-bit temporary key through the digitizer (output report
`0x6F`, Microsoft's *Seamless Pairing for Windows Pens Implementation Guide*). Windows then pairs
out-of-band with that key. The key never crosses the radio, so recording the pairing gets an
eavesdropper nothing.

Linux pairs the same pen with passkey entry and passkey 0 instead (docs/protocol/seamless-pairing.md
§3). The pen accepts that as an authenticated pairing, but the temporary key is public: someone who
records the radio during that one pairing second can derive the link key afterwards (legacy pairing
offers no protection against a passive eavesdropper unless the out-of-band method is used). Bonds
are only made once per pen and host, so the exposure is that one moment, but it is a real gap.

## What is missing

- The pen pairs **LE legacy** only (its Pairing Response has no Secure Connections bit), so the
  kernel's existing Secure Connections out-of-band support does not apply.
- `net/bluetooth/smp.c`: `build_pairing_cmd()` sets the OOB flag only on the Secure Connections
  path; on the legacy path `tk_request()` chooses Just Works or passkey entry and never takes a
  temporary key from outside.
- `net/bluetooth/mgmt.c`: *Add Remote Out Of Band Data* rejects non-zero `Hash_192`/`Randomizer_192`
  for LE addresses. Commit `d25b78e2ed` ("Bluetooth: Enforce zero-valued hash/rand192 for LE OOB",
  2015): "Until legacy SMP OOB pairing is implemented user space should be given a clear error".
- BlueZ `doc/mgmt-protocol.rst`, same command: "Currently there is no support for providing the
  Security Manager TK Value for LE legacy pairing."

The interface slot exists and is reserved; the feature was never implemented.

## Shape of a patch (to be designed with the maintainers)

1. `mgmt.c`: for LE address types, accept a legacy temporary key in the reserved P-192 slot (or
   wherever the maintainers prefer) and store it with the remote OOB data, instead of returning
   Invalid Parameters.
2. `smp.c`: on the legacy path, set `oob_flag = SMP_OOB_PRESENT` in the Pairing Request/Response when
   legacy OOB data exists for the peer; when both sides set it, select the OOB method, load that key
   as the temporary key, and treat the result as MITM-protected (the Core specification's legacy
   OOB association model).
3. Tests: the kernel's SMP self-tests and a pairing against a legacy OOB peer (this pen is one).
4. BlueZ: document the new LE meaning of the field in `doc/mgmt-protocol.rst`.

Process: clean-room from the Bluetooth Core specification and the public guide, follow the
linux-bluetooth list's patch rules and the kernel's AI-assistance policy, and test on real
hardware before posting. The maintainer submits; nothing is posted without that review.

## What changes in this project afterwards

Nothing for users. The daemon writes a fresh random key into `0x6F`, adds the same key as remote
out-of-band data (the pairing helper already speaks the management protocol with the needed
privilege), and pairs. On kernels without the feature it keeps using passkey 0. One package covers
both, and pairing gets the Windows level of protection as kernels pick the patch up.
