use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Result, Seek, SeekFrom, Write};
use std::marker::PhantomData;
use std::mem;
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, Mutex};
use struct_layout::StructLayout;

/// General comment:
/// I'm using GenAI heavily to assist in creating this. I may comment on certain decisions it makes
/// I also may leave some be. Leaving this comment because I may not always make clear I'm
/// explaining an AI's decision vs. a decision I made. If it is important I will attempt to make a
/// distinction
///
/// TODO Callouts:
/// * See the todo on the page cache. I think its totally busted right now and won't actually
/// update pages
/// * Replace all attempts to lock().unwrap() with something else cause that just seems like a
/// catastrophe waiting to happen
/// * Actually start doing checksumming. Right now I don't think any is happening

const DB_VERSION: u32 = 1;

struct PageWindow<'a, T> {
    header_bytes: &'a mut [u8],
    page_bytes: &'a mut [u8],
    _phantom: PhantomData<T>,
}

// TODO: Have this be where a checksum is done on reads
impl<'a, T> PageWindow<'a, T> {
    fn new(bytes: &'a mut Vec<u8>) -> Self {
        if bytes.len() < PageHeader::SIZE {
            panic!("invalid sequence of bytes")
        }
        let (header_bytes, page_bytes) = bytes.split_at_mut(PageHeader::SIZE);
        Self {
            header_bytes,
            page_bytes,
            _phantom: PhantomData,
        }
    }
}

// Page type enum
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageType {
    Metadata = 0,
    Data = 1,
    Index = 2,
    Overflow = 3,
    FreeList = 4,
}

impl PageType {
    fn to_be_bytes(&self) -> [u8; 1] {
        (*self as u8).to_be_bytes()
    }

    fn from_be_bytes(bytes: [u8; 1]) -> Self {
        match u8::from_be_bytes(bytes) {
            0 => Self::Metadata,
            1 => Self::Data,
            2 => Self::Index,
            3 => Self::Overflow,
            4 => Self::FreeList,
            // TODO: Is this bad practice?
            _ => panic!("Can't convert from bytes"),
        }
    }
}

impl From<u16> for PageType {
    fn from(value: u16) -> Self {
        match value {
            0 => PageType::Metadata,
            1 => PageType::Data,
            2 => PageType::Index,
            3 => PageType::Overflow,
            4 => PageType::FreeList,
            _ => PageType::Data, // Default
        }
    }
}

pub trait MySerialize {
    /// Trait used for serializing to a given buffer.
    /// Implemntations should ensure before attempting to write that the content they're going to
    /// write will fit into the provided buffer.
    ///
    /// The return value will be the total number of bytes written to the buffer.
    /// WARNING: It will not be memory aligned. Before writing any more bytes to the buffer passed
    /// the proper alignment should first be found
    fn serialize(&self, buffer: &mut [u8]) -> usize;
}

// Common header for all pages
#[repr(C)]
#[derive(StructLayout)]
pub struct PageHeader {
    pub page_id: u64,
    pub page_type: PageType,
    pub checksum: u32,
    pub lsn: u64,                // Log Sequence Number
    pub free_space_pointer: u32, // Pointer to start of free space in the page
}

impl MySerialize for PageHeader {
    fn serialize(&self, buffer: &mut [u8]) -> usize {
        let size_to_write = Self::free_space_pointer_span().end;
        if buffer.len() < size_to_write {
            panic!("Buffer too small for page header");
        }

        // Write fields in big-endian order
        // TODO: Ordering is chosen because "it makes it easier to sort keys because it maintains
        // lexiographical order". Maybe that will be useful. LittleEndian is what x86 uses though
        // so it may be more beneficial to just match that. Determine what's best here

        buffer[Self::page_id_span()].copy_from_slice(&self.page_id.to_be_bytes());
        buffer[Self::page_type_span()].copy_from_slice(&self.page_type.to_be_bytes());
        buffer[Self::checksum_span()].copy_from_slice(&self.checksum.to_be_bytes());
        buffer[Self::lsn_span()].copy_from_slice(&self.lsn.to_be_bytes());
        buffer[Self::free_space_pointer_span()]
            .copy_from_slice(&self.free_space_pointer.to_be_bytes());

        size_to_write
    }
}

impl PageHeader {
    pub fn new(page_id: u64, page_type: PageType) -> Self {
        PageHeader {
            page_id,
            page_type,
            checksum: 0,
            lsn: 0,
            // This is fine to be Self::Size as its where the padding ends for the struct
            free_space_pointer: Self::SIZE as u32, // Initially points to end of header
        }
    }

    pub fn size() -> usize {
        Self::free_space_pointer_span().end
    }

    pub fn deserialize(buffer: Vec<u8>) -> Self {
        let size_to_read = Self::free_space_pointer_span().end;
        if buffer.len() < size_to_read {
            panic!("Buffer too small for page header");
        }

        let mut page_id_buffer = [0u8; Self::PAGE_ID_SIZE];
        page_id_buffer.copy_from_slice(&buffer[Self::page_id_span()]);
        let page_id = u64::from_be_bytes(page_id_buffer);

        let mut page_type_buffer = [0u8; Self::PAGE_TYPE_SIZE];
        page_type_buffer.copy_from_slice(&buffer[Self::page_type_span()]);
        let page_type = PageType::from_be_bytes(page_type_buffer);

        let mut checksum_buffer = [0u8; Self::CHECKSUM_SIZE];
        checksum_buffer.copy_from_slice(&buffer[Self::checksum_span()]);
        let checksum = u32::from_be_bytes(checksum_buffer);

        let mut lsn_buffer = [0u8; Self::LSN_SIZE];
        lsn_buffer.copy_from_slice(&buffer[Self::lsn_span()]);
        let lsn = u64::from_be_bytes(lsn_buffer);

        let mut free_space_pointer_buffer = [0u8; Self::FREE_SPACE_POINTER_SIZE];
        free_space_pointer_buffer.copy_from_slice(&buffer[Self::free_space_pointer_span()]);
        let free_space_pointer = u32::from_be_bytes(free_space_pointer_buffer);

        PageHeader {
            page_id,
            page_type,
            checksum,
            lsn,
            free_space_pointer,
        }
    }
}

// Metadata page structure
#[repr(C)]
#[derive(StructLayout)]
pub struct MetadataPage {
    pub db_version: u32,
    pub page_size: u32,
    pub root_page_id: u64,
    /// Free list page is a page that can be freed. I.E one that has been marked for deletion.
    /// The contents of that page will be the next page marked for deletion. So all that's needed
    /// to start clearing page is the index of the first page
    pub first_free_list_page: u64,
    pub total_pages: u64,
}

impl MySerialize for MetadataPage {
    fn serialize(&self, buffer: &mut [u8]) -> usize {
        let size_to_write = Self::total_pages_span().end;
        if buffer.len() < size_to_write {
            panic!("Buffer too small for page header");
        }

        buffer[Self::db_version_span()].copy_from_slice(&self.db_version.to_be_bytes());
        buffer[Self::page_size_span()].copy_from_slice(&self.page_size.to_be_bytes());
        buffer[Self::root_page_id_span()].copy_from_slice(&self.root_page_id.to_be_bytes());
        buffer[Self::first_free_list_page_span()]
            .copy_from_slice(&self.first_free_list_page.to_be_bytes());
        buffer[Self::total_pages_span()].copy_from_slice(&self.total_pages.to_be_bytes());

        Self::total_pages_span().end
    }
}

impl MetadataPage {
    pub fn intial_page(page_size: u32) -> Self {
        MetadataPage {
            db_version: DB_VERSION,
            page_size,
            root_page_id: 0,
            first_free_list_page: 0,
            total_pages: 1, // Just this metadata page initially
        }
    }
}

impl<'a> PageWindow<'a, MetadataPage> {
    fn read_total_pages(&self) -> u64 {
        let mut u64_bytes = [0u8; size_of::<u64>()];
        u64_bytes.copy_from_slice(&self.page_bytes[MetadataPage::total_pages_span()]);
        u64::from_be_bytes(u64_bytes)
    }

    fn update_total_pages(&mut self, new_total_pages: u64) {
        self.page_bytes[MetadataPage::total_pages_span()]
            .copy_from_slice(&new_total_pages.to_be_bytes());
    }
}

// Data page structure
#[repr(C)]
#[derive(StructLayout)]
pub struct DataPage {
    pub num_records: u32,
    // Offsets to records within the page
    // TODO: Determine from where in
    // the page this offset should start from. For now I'm assuming its
    // the start of the page itself
    pub slot_array: Vec<u32>,
}

impl MySerialize for DataPage {
    fn serialize(&self, buffer: &mut [u8]) -> usize {
        let size = self.size();
        if buffer.len() < size {
            panic!("Buffer too small for data page");
        }
        // Write num_records
        buffer[Self::num_records_span()].copy_from_slice(&self.num_records.to_be_bytes());

        // Write slot array length
        let slot_array_len = self.slot_array.len() as u32;
        buffer[Self::slot_array_length_span()].copy_from_slice(&slot_array_len.to_be_bytes());

        // Write slot array entries
        let mut current_offset = Self::SLOT_ARRAY_FIRST_VALUE_OFFSET;
        for &slot_offset in &self.slot_array {
            buffer[current_offset..current_offset + Self::SLOT_ARRAY_VALUE_SIZE]
                .copy_from_slice(&slot_offset.to_be_bytes());
            current_offset += Self::SLOT_ARRAY_VALUE_SIZE;
        }

        assert!(size == current_offset);
        size
    }
}

impl DataPage {
    const SLOT_ARRAY_LEN_SIZE: usize = size_of::<u32>();
    const SLOT_ARRAY_VALUE_SIZE: usize = size_of::<u32>();

    const SLOT_ARRAY_LEN_OFFSET: usize = Self::NUM_RECORDS_OFFSET + Self::NUM_RECORDS_SIZE;
    const SLOT_ARRAY_FIRST_VALUE_OFFSET: usize =
        // I don't think the padding does anything here but keeping it just in case
        padding_needed_from_type::<u32>(
            Self::SLOT_ARRAY_LEN_OFFSET + Self::SLOT_ARRAY_LEN_SIZE,
        ) + Self::SLOT_ARRAY_LEN_OFFSET
            + Self::SLOT_ARRAY_LEN_SIZE;

    const MIN_SIZE: usize = Self::SLOT_ARRAY_FIRST_VALUE_OFFSET;

    fn slot_array_length_span() -> Range<usize> {
        Self::SLOT_ARRAY_LEN_OFFSET..Self::SLOT_ARRAY_LEN_OFFSET + Self::SLOT_ARRAY_LEN_SIZE
    }

    pub fn size(&self) -> usize {
        Self::MIN_SIZE + (Self::SLOT_ARRAY_VALUE_SIZE * self.slot_array.len())
    }

    pub fn new() -> Self {
        DataPage {
            num_records: 0,
            slot_array: Vec::new(),
        }
    }
}

// Index page structure
#[repr(C)]
#[derive(StructLayout)]
pub struct IndexPage {
    pub is_leaf: bool,
    // Only used if is_leaf is true
    pub next_leaf: u64,
    // Variable-length keys. The variable is across index pages. One index page will have keys that
    // all are the same size otherwise panics will occur
    //
    // WARNING (or TODO): Right now the code assumes that the bytes represenging the keys will be
    // in BigEndian here. Need to confirm that somehow
    pub keys: Vec<Vec<u8>>,
    // Page IDs for children
    pub child_pointers: Vec<u64>,
}

impl MySerialize for IndexPage {
    fn serialize(&self, buffer: &mut [u8]) -> usize {
        // TODO: Determine if these calcuations are worth it. I think they are? but need to confirm
        // later if I even really care about checking initial buffer size
        let size = self.calc_size();
        if buffer.len() < size {
            panic!("Buffer too small for index page");
        }

        buffer[Self::is_leaf_span()].copy_from_slice(&if self.is_leaf { [1u8] } else { [0u8] });
        buffer[Self::next_leaf_span()].copy_from_slice(&self.next_leaf.to_be_bytes());

        buffer[Self::KEYS_LEN_OFFSET..Self::KEYS_LEN_SIZE]
            .copy_from_slice(&self.keys.len().to_be_bytes());
        let key_size_opt = self.key_value_size();
        let mut current_key_offset = Self::KEYS_FIRST_VALUE_OFFSET_WITHOUT_PADDING;
        for key in self.keys.iter() {
            match key_size_opt {
                Some(key_size) if key_size == key.len() => {
                    // Add the padding necessary
                    current_key_offset += padding_needed_from_size(current_key_offset, key_size);
                    // Write to the buffer
                    buffer[current_key_offset..current_key_offset + key_size]
                        .copy_from_slice(&key.as_slice());
                    // Increment by amount written to buffer. We don't do the padding after because
                    // we don't know if we're going to write another key or we're going to write
                    // the len of the next vec
                    current_key_offset += key_size;
                }
                Some(key_size) if key_size != key.len() => {
                    panic!("varying key sizes")
                }
                _ => panic!("key found but no key size found"),
            }
        }
        // We add the padding from the last key needed before writing the len of the child pointers
        // vec
        current_key_offset += padding_needed_from_type::<u32>(current_key_offset);
        // Write the child pointer vec
        buffer[current_key_offset..current_key_offset + Self::CHILD_POINTERS_LEN_SIZE]
            .copy_from_slice(&self.child_pointers.len().to_be_bytes());
        let mut current_child_pointer_offset = current_key_offset + Self::CHILD_POINTERS_LEN_SIZE;
        // TODO: This might not be needed, its unclear to me if this can be determined statically
        // or not
        current_child_pointer_offset +=
            padding_needed_from_type::<u64>(current_child_pointer_offset);
        for child_pointer in self.child_pointers.iter() {
            buffer[current_child_pointer_offset
                ..current_child_pointer_offset + Self::CHILD_POINTERS_VALUE_SIZE]
                .copy_from_slice(&child_pointer.to_be_bytes());
            current_child_pointer_offset += Self::CHILD_POINTERS_VALUE_SIZE;
        }

        assert!(size == current_child_pointer_offset);
        size
    }
}

impl IndexPage {
    const KEYS_LEN_SIZE: usize = size_of::<u32>();
    const CHILD_POINTERS_LEN_SIZE: usize = size_of::<u32>();
    const CHILD_POINTERS_VALUE_SIZE: usize = size_of::<u64>();

    const KEYS_LEN_OFFSET: usize =
        // Next type is the key len TODO: I think that this padding does nothing
        padding_needed_from_type::<u32>(Self::NEXT_LEAF_OFFSET + Self::NEXT_LEAF_SIZE)
                + Self::NEXT_LEAF_OFFSET
                + Self::NEXT_LEAF_SIZE;
    // Don't know how much padding is needed because we don't know how big the keys are yet
    const KEYS_FIRST_VALUE_OFFSET_WITHOUT_PADDING: usize =
        Self::KEYS_LEN_OFFSET + Self::KEYS_LEN_SIZE;

    const EXCLUDING_VEC_SIZE: usize = Self::KEYS_LEN_OFFSET;

    // Assumes that all elements of the key vec will be the same size. That properly has to be
    // checked when calling this method. I.E this method does not confirm that fact.
    //
    // Some(value) -> The key vec has a key and its size along
    // None -> The key vec has no keys
    fn key_value_size(&self) -> Option<usize> {
        self.keys.first().map(|first_key| first_key.len())
    }

    /// Does NOT include any padding after the final value. This is because the contract of
    /// serialize is to not include that padding cause it doesn't know what the next value is going
    /// to be
    pub fn calc_size(&self) -> usize {
        let size_of_key_vec = match self.key_value_size() {
            Some(key_size) => {
                let total_size_of_keys = key_size * self.keys.len();
                let padding_to_first_key =
                    padding_needed_from_size(Self::EXCLUDING_VEC_SIZE, key_size);
                let total_padding_between_keys =
                    padding_needed_from_size(key_size, key_size) * (self.keys.len() - 1);

                total_size_of_keys + padding_to_first_key + total_padding_between_keys
            }
            None => 0,
        };

        let offset_after_key_vec = Self::EXCLUDING_VEC_SIZE + size_of_key_vec;
        let offset_after_child_pointers_key_len =
            padding_needed_from_type::<u32>(offset_after_key_vec)
                + offset_after_key_vec
                + Self::CHILD_POINTERS_LEN_SIZE;

        let padding_to_first_child_pointer =
            padding_needed_from_type::<u64>(offset_after_child_pointers_key_len);
        let size_of_child_pointers = Self::CHILD_POINTERS_VALUE_SIZE * self.child_pointers.len();

        offset_after_child_pointers_key_len
            + padding_to_first_child_pointer
            + size_of_child_pointers
    }

    pub fn new(is_leaf: bool) -> Self {
        IndexPage {
            is_leaf,
            next_leaf: 0,
            keys: Vec::new(),
            child_pointers: Vec::new(),
        }
    }
}

// FreeList page structure
// TODO: Confirm this pages impl looks correct
#[repr(C)]
#[derive(StructLayout)]
pub struct FreeListPage {
    pub next_free_list: u64,
    pub free_page_ids: Vec<u64>,
}

impl FreeListPage {
    const FREE_PAGE_IDS_LEN_SIZE: usize = size_of::<u32>();
    const FREE_PAGE_IDS_VALUE_SIZE: usize = size_of::<u64>();

    const FREE_PAGE_IDS_LEN_OFFSET: usize = Self::NEXT_FREE_LIST_OFFSET + Self::NEXT_FREE_LIST_SIZE;
    const FREE_PAGE_IDS_FIRST_VALUE_OFFSET: usize = padding_needed_from_type::<u64>(
        Self::FREE_PAGE_IDS_LEN_OFFSET + Self::FREE_PAGE_IDS_LEN_SIZE,
    ) + Self::FREE_PAGE_IDS_LEN_OFFSET
        + Self::FREE_PAGE_IDS_LEN_SIZE;

    const MIN_SIZE: usize = Self::FREE_PAGE_IDS_FIRST_VALUE_OFFSET;

    pub fn new() -> Self {
        FreeListPage {
            next_free_list: 0,
            free_page_ids: Vec::new(),
        }
    }

    pub fn serialize(&self, buffer: &mut [u8]) -> usize {
        let size_to_write =
            Self::MIN_SIZE + (Self::FREE_PAGE_IDS_VALUE_SIZE * self.free_page_ids.len());
        if buffer.len() < size_to_write {
            panic!("Buffer too small for index page");
        }

        buffer[Self::next_free_list_span()].copy_from_slice(&self.next_free_list.to_be_bytes());
        buffer[Self::FREE_PAGE_IDS_LEN_OFFSET
            ..Self::FREE_PAGE_IDS_LEN_OFFSET + Self::FREE_PAGE_IDS_LEN_SIZE]
            .copy_from_slice(&self.free_page_ids.len().to_be_bytes());
        let mut free_page_id_offset = Self::FREE_PAGE_IDS_FIRST_VALUE_OFFSET;
        for free_page_id in self.free_page_ids.iter() {
            buffer[free_page_id_offset..free_page_id_offset + Self::FREE_PAGE_IDS_VALUE_SIZE]
                .copy_from_slice(&free_page_id.to_be_bytes());
            free_page_id_offset += Self::FREE_PAGE_IDS_VALUE_SIZE;
        }

        assert!(size_to_write == free_page_id_offset);
        size_to_write
    }
}
pub struct PagedFileManagerConfig {
    page_size: u32,
    max_cache_size: usize,
}

#[derive(Default)]
pub struct PagedFileManagerConfigBuilder {
    page_size: Option<u32>,
    max_cache_size: Option<usize>,
}

impl PagedFileManagerConfigBuilder {
    const DEFAULT_PAGE_SIZE: u32 = 4096;
    const DEFAULT_MAX_CACHE_SIZE: usize = 100;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn page_size(mut self, size: u32) -> Self {
        self.page_size = Some(size);
        self
    }

    pub fn max_cache_size(mut self, size: usize) -> Self {
        self.max_cache_size = Some(size);
        self
    }

    pub fn build(self) -> PagedFileManagerConfig {
        PagedFileManagerConfig {
            page_size: self.page_size.unwrap_or(Self::DEFAULT_PAGE_SIZE),
            max_cache_size: self.max_cache_size.unwrap_or(Self::DEFAULT_MAX_CACHE_SIZE),
        }
    }
}

// File manager to handle page operations
pub struct PagedFileManager {
    file: Arc<Mutex<File>>,
    page_size: u32,
    buffer_pool: HashMap<u64, Vec<u8>>, // pageId -> raw page data
    max_cache_size: usize,
}

impl PagedFileManager {
    const METADATA_PAGE_ID: u64 = 0;

    pub fn new<P: AsRef<Path>>(path: P, config: PagedFileManagerConfig) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        let manager = PagedFileManager {
            file: Arc::new(Mutex::new(file)),
            page_size: config.page_size,
            buffer_pool: HashMap::new(),
            max_cache_size: config.max_cache_size,
        };

        // Initialize the file if it's new (create metadata page)
        let file_len = manager.file.lock().unwrap().metadata()?.len();
        if file_len == 0 {
            manager.initialize_file()?;
        }

        Ok(manager)
    }

    fn initialize_file(&self) -> Result<()> {
        // Create a buffer for the metadata page
        let mut page_buffer = vec![0u8; self.page_size as usize];

        // Create and write header
        let header = PageHeader::new(Self::METADATA_PAGE_ID, PageType::Metadata);
        let end_of_header = header.serialize(&mut page_buffer);
        // TODO: I think padding the whole MetadataPage is fine? Rather than just its first value
        let metadata_offset = padding_needed_from_type::<MetadataPage>(end_of_header);

        // TODO: This can probably be a debug_assert
        assert!(metadata_offset == PageHeader::SIZE);

        let metadata_page = MetadataPage::intial_page(self.page_size);
        metadata_page.serialize(&mut page_buffer[metadata_offset..]);

        // Write to file
        let mut file = self.file.lock().unwrap();
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&page_buffer)?;
        file.sync_all()?;

        Ok(())
    }

    pub fn allocate_page(&mut self) -> Result<u64> {
        // Read metadata to get next page ID
        let cache = &mut self.buffer_pool;
        let mut page_bytes = Self::load_into_buffer_pool(
            cache,
            self.max_cache_size,
            Self::METADATA_PAGE_ID,
            // TODO: Does self.file.clone() do anything weird here. It ~feels~ wrong.
            || Self::read_page(self.file.clone(), Self::METADATA_PAGE_ID, self.page_size),
        )?;
        let mut metadata_page_window = PageWindow::<MetadataPage>::new(&mut page_bytes);

        let new_page_id = metadata_page_window.read_total_pages() + 1;
        metadata_page_window.update_total_pages(new_page_id);

        // Write updated metadata page
        let to_write = mem::take(page_bytes);
        self.write_page(Self::METADATA_PAGE_ID, to_write)?;

        // Create empty page
        let empty_page = vec![0u8; self.page_size as usize];
        self.write_page(new_page_id, empty_page)?;

        Ok(new_page_id)
    }

    fn read_page(file: Arc<Mutex<File>>, page_id: u64, page_size: u32) -> Result<Vec<u8>> {
        // Read from disk
        let mut page_data = vec![0u8; page_size as usize];
        let mut file = file.lock().unwrap();
        file.seek(SeekFrom::Start(page_id * page_size as u64))?;
        file.read_exact(&mut page_data)?;

        Ok(page_data)
    }

    pub fn load_into_buffer_pool<F>(
        cache: &mut HashMap<u64, Vec<u8>>,
        max_cache_size: usize,
        page_id: u64,
        loader: F,
    ) -> Result<&mut Vec<u8>>
    where
        F: FnOnce() -> Result<Vec<u8>>,
    {
        // Check cache first
        if !cache.contains_key(&page_id) {
            let page_data = loader()?;
            // Update cache TODO: This might be needed elsewhere too, but I need to implement some sort of
            // mechanism for locking cache keys so that I can guarantee this won't break things
            if cache.len() >= max_cache_size {
                // Simple eviction - remove first key
                if let Some(key) = cache.keys().next().cloned() {
                    cache.remove(&key);
                }
            }
            cache.insert(page_id, page_data);
        }

        // TODO: Find a way to make it guaranteed this will be in the cache at this point
        // I think the best way is to find a locking mechanism based on key
        Ok(cache.get_mut(&page_id).unwrap())
    }

    pub fn write_page(&self, page_id: u64, data: Vec<u8>) -> Result<()> {
        // Write to disk
        let mut file = self.file.lock().unwrap();
        file.seek(SeekFrom::Start(page_id * self.page_size as u64))?;
        file.write_all(&data)?;
        file.sync_all()?;
        Ok(())
    }

    //
    // Creating specific page types
    //

    pub fn create_data_page(&mut self) -> Result<u64> {
        let page_id = self.allocate_page()?;
        let mut page_buffer = vec![0u8; self.page_size as usize];

        let mut header = PageHeader::new(page_id, PageType::Data);
        let data_page = DataPage::new();

        // u32 is the first datatype of DataPage
        let data_page_offset =
            PageHeader::size() + padding_needed_from_type::<u32>(PageHeader::size());
        // TODO: This is dangerous I think but realistically it should never panic
        header.free_space_pointer = (data_page_offset + data_page.size()) as u32;

        let initial_offset = header.serialize(&mut page_buffer);
        let offset_with_padding = initial_offset + padding_needed_from_type::<u32>(initial_offset);
        assert!(offset_with_padding == data_page_offset);
        let final_offset = data_page.serialize(&mut page_buffer[offset_with_padding..]);

        assert!(final_offset == header.free_space_pointer as usize);

        self.write_page(page_id, page_buffer)?;

        Ok(page_id)
    }

    pub fn create_index_page(&mut self, is_leaf: bool) -> Result<u64> {
        let page_id = self.allocate_page()?;
        let mut page_buffer = vec![0u8; self.page_size as usize];

        // Initialize header
        let mut header = PageHeader::new(page_id, PageType::Index);
        let index_page = IndexPage::new(is_leaf);

        let index_page_offset =
            PageHeader::size() + padding_needed_from_type::<bool>(PageHeader::size());
        // TODO: This is dangerous I think but realistically it should never panic
        header.free_space_pointer = (index_page_offset + index_page.calc_size()) as u32;

        let initial_offset = header.serialize(&mut page_buffer);
        let offset_with_padding = initial_offset + padding_needed_from_type::<bool>(initial_offset);
        assert!(offset_with_padding == index_page_offset);
        let final_size = index_page.serialize(&mut page_buffer[offset_with_padding..]);

        assert!(final_size == header.free_space_pointer as usize);

        self.write_page(page_id, page_buffer)?;

        Ok(page_id)
    }
}

const fn padding_needed_from_size(offset: usize, next_size: usize) -> usize {
    // For most primitive types, alignment equals size
    // But we cap at common max alignments and handle special cases
    let alignment = match next_size {
        0 => 1,       // Zero-sized types still need 1-byte alignment
        1 => 1,       // u8/i8 need 1-byte alignment
        2 => 2,       // u16/i16 need 2-byte alignment
        3..=4 => 4,   // u32/i32/f32 need 4-byte alignment
        5..=8 => 8,   // u64/i64/f64 need 8-byte alignment
        9..=16 => 16, // Large types like u128/i128 often need 16-byte alignment
        _ => 16,      // Default to conservative alignment for larger types
    };
    let remainder = offset % alignment;

    gen_padding(alignment, remainder)
}

const fn padding_needed_from_type<T>(offset: usize) -> usize {
    let alignment = mem::align_of::<T>();
    let remainder = offset % alignment;
    gen_padding(alignment, remainder)
}

const fn gen_padding(alignment: usize, remainder: usize) -> usize {
    if remainder == 0 {
        0 // Already aligned
    } else {
        alignment - remainder // Padding needed to reach alignment
    }
}

// Initial stuff
pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
