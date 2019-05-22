# Purpose

This file describes the precise binary format for each message.
Protocol definitions are to be found more precisely in
[the original kademlia paper](https://pdos.csail.mit.edu/~petar/papers/maymounkov-kademlia-lncs.pdf)

The paper only specifies the outgoing rpc calls, and not the responses,
which we do here.

Unless otherwise specified, all numbers are in network (Big-endian) byte order.

## Message Format

Each RPC call or response is prefixed with a header, specified as follows:
|field|size (bytes)|description    |
|-----|------------|---------------|
|node_id|16|the ID of the sender|
|transaction_id|8|an identifier for this call|

The transaction ID is used to link together RPC calls and responses 
correctly, and to mitigate IP spoofing. The initiator of an RPC call
generates a random transaction ID, and includes it in their call. The
response to that call must then include that transaction ID.

After the header, the rest of the message depends on the specific RPC
call or response.

## Ping

### Request
|field|size (bytes)|description    |
|-----|------------|---------------|
|type|1|0x1 for Ping Request|

### Response
|field|size (bytes)|description    |
|-----|------------|---------------|
|type|1|0x2 for Ping Response|

## FindNode

### Request
|field|size (bytes)|description    |
|-----|------------|---------------|
|type|1|0x3 for FindNode request|
|find_id|16|the id of the node to search for|

### Response
|field|size (bytes)|description    |
|-----|------------|---------------|
|type|1|0x4 for FindNode Response|
|node_count|1|(u8) how times the next field appears|
|node_id[i]|16|the id of the ith node returned|
|ip_type|1|0x4 for IPV4 and 0x6 for IPV6|
|addr[i]|16 / 4|16 bytes for IPV6, 4 for IPV4|
|port[i]|2|the 16 bit port for this node|

## Store

### Request
|field|size (bytes)|description    |
|-----|------------|---------------|
|type|1|0x5 for Store request|
|key_len|1|(u8) how long the next field is|
|key|key_len|the string key|
|val_len|1|(u8) how long the next field is|
|val|val_len|the value to associate with this key|

### Response
|field|size (bytes)|description    |
|-----|------------|---------------|
|type|1|0x6 for Store response|

## FindValue

Find value is different in that the RPC call either returns
a single string if the value associated with a key was found,
and otherwise returns a response similar to that of FindNode.

### Request
|field|size (bytes)|description    |
|-----|------------|---------------|
|type|1|0x7 for FindValue Request|
|key_len|1|(u8) how long the next field is|
|key|key_len|the string key we want to find|

### Node Response
|field|size (bytes)|description    |
|-----|------------|---------------|
|type|1|0x8 for FindValue Node Response|
|node_count|1|(u8) how times the next field appears|
|node_id[i]|16|the id of the ith node returned|
|ip_type|1|0x4 for IPV4 and 0x6 for IPV6|
|addr[i]|16 / 4|16 bytes for IPV6, 4 for IPV4|
|port[i]|2|the 16 bit port for this node|

## Value Response
|field|size (bytes)|description    |
|-----|------------|---------------|
|type|1|0x9 for FindValue Value Response|
|val_len|1|(u8) how long the next field is|
|val|val_len|the value for the key we requested|
