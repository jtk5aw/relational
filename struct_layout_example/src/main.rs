use memoffset::span_of;
use struct_layout::StructLayout;

// Define an enum for demonstration
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
enum Status {
    Inactive = 0,
    Active = 1,
    Pending = 2,
    Error = 255,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
enum Category {
    Personal = 1,
    Work = 2,
    Social = 3,
    Other = 100,
}

#[repr(C)]
#[derive(StructLayout, Debug, Clone, Copy)]
struct Person {
    id: u64,
    age: u16,
    height: f32,
    weight: f64,
    is_active: bool,
}

#[repr(C)]
#[derive(StructLayout, Debug, Clone, Copy)]
struct Point {
    x: f32,
    y: f32,
    z: f32,
}

// A struct with enums
#[repr(C)]
#[derive(StructLayout, Debug, Clone, Copy)]
struct Task {
    id: u32,
    status: Status,     // Enum field
    category: Category, // Another enum field
    priority: u8,
}

#[repr(C)]
#[derive(StructLayout)]
struct Test {
    a: u64,
    b: u32,
}

// A struct with mixed types including enums
#[repr(C)]
#[derive(StructLayout, Debug, Clone)]
struct MixedWithEnum {
    id: u64,
    name: String,   // Non-primitive, should be ignored
    status: Status, // Enum, should be included
    data: Vec<u8>,  // Non-primitive, should be ignored
}

// Won't compile
// #[repr(C)]
// #[derive(StructLayout)]
// struct NoCompile {
//     vec: Vec<u64>,
//     id: u64,
// }

fn main() {
    // Print information about the Person struct (all primitives)
    println!("Person struct layout information:");
    println!("Total size: {} bytes", Person::SIZE);
    println!("Field count: {}", Person::FIELD_COUNT);
    println!();

    println!("Field offsets (as constants):");
    println!("id: {} bytes", Person::ID_OFFSET);
    println!("age: {} bytes", Person::AGE_OFFSET);
    println!("height: {} bytes", Person::HEIGHT_OFFSET);
    println!("weight: {} bytes", Person::WEIGHT_OFFSET);
    println!("is_active: {} bytes", Person::IS_ACTIVE_OFFSET);
    println!();

    println!("Field sizes (as constants):");
    println!("id: {} bytes", Person::ID_SIZE);
    println!("age: {} bytes", Person::AGE_SIZE);
    println!("height: {} bytes", Person::HEIGHT_SIZE);
    println!("weight: {} bytes", Person::WEIGHT_SIZE);
    println!("is_active: {} bytes", Person::IS_ACTIVE_SIZE);
    println!();

    println!("Field spans (as methodss):");
    println!("id: {:?}", Person::id_span());
    println!("age: {:?}", Person::age_span());
    println!("height: {:?}", Person::height_span());
    println!("weight: {:?}", Person::weight_span());
    println!("is_active: {:?}", Person::is_active_span());
    println!();

    // Print information about the Task struct (primitives and enums)
    println!("\nTask struct layout information:");
    println!("Total size: {} bytes", Task::SIZE); // Available since all fields are primitives or enums
    println!("Field count: {}", Task::FIELD_COUNT);
    println!();

    println!("Field offsets (as constants):");
    println!("id: {} bytes", Task::ID_OFFSET);
    println!("status: {} bytes", Task::STATUS_OFFSET); // Enum field
    println!("category: {} bytes", Task::CATEGORY_OFFSET); // Enum field
    println!("priority: {} bytes", Task::PRIORITY_OFFSET);
    println!();

    println!("Field sizes (as constants):");
    println!("id: {} bytes", Task::ID_SIZE);
    println!("status: {} bytes", Task::STATUS_SIZE); // Enum field, should be 1 byte (u8)
    println!("category: {} bytes", Task::CATEGORY_SIZE); // Enum field, should be 4 bytes (u32)
    println!("priority: {} bytes", Task::PRIORITY_SIZE);
    println!();

    println!("Field spans (as methods):");
    println!("id: {:?}", Task::id_span());
    println!("status: {:?}", Task::status_span()); // Enum field
    println!("category: {:?}", Task::category_span()); // Enum field
    println!("priority: {:?}", Task::priority_span());
    println!();

    // Example with Task struct
    let task = Task {
        id: 12345,
        status: Status::Active,
        category: Category::Work,
        priority: 3,
    };

    // Allocate a buffer for serialization using the SIZE constant
    let mut buffer = vec![0u8; Task::SIZE];

    // Manually serialize each field
    buffer[Task::id_span()].copy_from_slice(&task.id.to_be_bytes());
    buffer[Task::status_span()][0] = task.status as u8; // Status enum as u8
    buffer[Task::category_span()].copy_from_slice(&(task.category as u32).to_be_bytes()); // Category enum as u32
    buffer[Task::priority_span()][0] = task.priority;

    println!("Manually serialized task: {:?}", buffer);

    // Print information about the Test struct
    println!("\nTest struct layout information:");
    println!("Total size: {} bytes", Test::SIZE); // Available since all fields are primitives or enums
    println!("Field count: {}", Test::FIELD_COUNT);
    println!();

    println!("Field offsets (as constants):");
    println!("id: {} bytes", Test::A_OFFSET);
    println!("status: {} bytes", Test::B_OFFSET);
    println!();

    println!("Field sizes (as constants):");
    println!("id: {} bytes", Test::A_SIZE);
    println!("status: {} bytes", Test::B_SIZE);
    println!();

    println!("Field spans (as methods):");
    println!("id: {:?}", Test::a_span());
    println!("status: {:?}", Test::b_span());
    println!();

    // Test with mixed types including enums
    println!("\nMixedWithEnum struct layout information:");
    println!(
        "Primitive & enum field count: {}",
        MixedWithEnum::FIELD_COUNT
    );
    println!();

    // Only primitive and enum fields have layout information
    println!("id offset: {} bytes", MixedWithEnum::ID_OFFSET);
    println!("id size: {} bytes", MixedWithEnum::ID_SIZE);
    println!("id span: {:?}", MixedWithEnum::id_span());

    println!(
        "status (enum) offset: {} bytes",
        MixedWithEnum::STATUS_OFFSET
    );
    println!("status (enum) size: {} bytes", MixedWithEnum::STATUS_SIZE);
    println!("status (enum) span: {:?}", MixedWithEnum::status_span());

    // Note: No constants or methods for 'name' and 'data' because they're not primitives or enums
}
