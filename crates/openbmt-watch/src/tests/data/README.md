# Test data

`mainnet-headers-967680.bin` holds 4,043 raw 80-byte Bitcoin mainnet block
headers, one after the other, for heights 967,680 to 971,722. The first is
the header the header store compiles in as its mainnet checkpoint. The file
crosses two difficulty changes, at heights 969,696 and 971,712.

The file is 323,440 bytes. Its sha256 is
`f13ba75b891e814b6f091507d436ec0b20c50631070943df345220f65f4e793c`.

It can be reproduced from any Bitcoin Core node: for each height, take the
block hash from `getblockhash <height>` and the header from
`getblockheader <hash> false`, which prints the 80 bytes in hex.
