# SCRATCHING USING THIS FOR NOW. TOO MANY OF THE FIELDS WOULD NEED TO BE UPDATED
# IN PLACE WITHOUT UPDATING THE ENTIRE STRUCT SO I'M GOING TO SKIP THIS FOR NOW
# MIGHT BE USEFUL IN OTHER PARTS THOUGH
@0x94f0dadfe6bd7bb7; # Generated using capnp id

struct PageHeader {
  pageId @0 :UInt64;
  pageType @1 :UInt16;
  checksum @2 :UInt32;
  lsn @3 :UInt64;  # Log Sequence Number
  freeSpacePointer @4 :UInt32;
}

enum PageType {
  data @0;
  index @1;
  overflow @2;
  freelist @3;
  metadata @4;
}

struct DataPage {
  header @0 :PageHeader;
  numRecords @1 :UInt32;
  records @2 :List(Data);  # Raw record data
  slotArray @3 :List(UInt32);  # Offsets to records
}

struct IndexPage {
  header @0 :PageHeader;
  isLeaf @1 :Bool;
  numKeys @2 :UInt32;
  keys @3 :List(Data);  # Key data
  childPointers @4 :List(UInt64);  # Page IDs for children or record pointers
  nextLeaf @5 :UInt64;  # Next leaf for range scans (0 if none)
}

struct FreeListPage {
  header @0 :PageHeader;
  nextFreeList @1 :UInt64;  # Next freelist page (0 if none)
  freePageIds @2 :List(UInt64);  # Available page IDs
}

struct MetadataPage {
  header @0 :PageHeader;
  dbVersion @1 :UInt32;
  pageSize @2 :UInt32;
  rootPageId @3 :UInt64;  # Root of the B+Tree
  firstFreeListPage @4 :UInt64;
  totalPages @5 :UInt64;
}
