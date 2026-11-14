@0x896ff7adf3208565;

struct Entry { account @0 :UInt64; amount @1 :Int64; memo @2 :Text; }
struct Transaction {
  id @0 :UInt64; ts @1 :UInt64; currency @2 :UInt16; fromShard @3 :UInt32; toShard @4 :UInt32;
  entries @5 :List(Entry); note @6 :Text;
}

# Groups are Cap'n Proto's inline structs (no pointer).
struct Entity {
  id @0 :UInt32;
  pos :group { x @1 :Float32; y @2 :Float32; z @3 :Float32; }
  vel :group { x @4 :Float32; y @5 :Float32; z @6 :Float32; }
  hp @7 :UInt16;
  alive @8 :Bool;
}
struct Frame { tick @0 :UInt64; entities @1 :List(Entity); }

struct Telemetry {
  device @0 :UInt32; ts @1 :UInt64; temp @2 :Float32;
  humidity :union { none @3 :Void; value @4 :Float32; }
  battery @5 :UInt8;
}

struct Chunk { streamId @0 :UInt64; seq @1 :UInt32; data @2 :Data; }
