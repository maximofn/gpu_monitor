import Foundation

// Mirror of crates/gpu-monitor-core/src/model.rs. The Rust types are the
// canonical schema (API path /v1/...). If a field is added there, replicate it
// here verbatim or the JSON decode will silently drop data.

struct Snapshot: Codable, Equatable, Sendable {
    let timestamp: String
    let host: String
    let driverVersion: String?
    let cudaVersion: String?
    let gpus: [GPU]

    enum CodingKeys: String, CodingKey {
        case timestamp
        case host
        case driverVersion = "driver_version"
        case cudaVersion = "cuda_version"
        case gpus
    }
}

struct GPU: Codable, Equatable, Sendable {
    let index: UInt32
    let uuid: String
    let name: String
    let temperatureC: UInt32?
    let fanSpeedPercent: UInt32?
    let powerDrawW: Float?
    let powerLimitW: Float?
    let utilization: Utilization
    let memory: Memory
    let processes: [GPUProcess]

    enum CodingKeys: String, CodingKey {
        case index
        case uuid
        case name
        case temperatureC = "temperature_c"
        case fanSpeedPercent = "fan_speed_percent"
        case powerDrawW = "power_draw_w"
        case powerLimitW = "power_limit_w"
        case utilization
        case memory
        case processes
    }
}

struct Utilization: Codable, Equatable, Sendable {
    let gpuPercent: UInt32
    let memoryPercent: UInt32

    enum CodingKeys: String, CodingKey {
        case gpuPercent = "gpu_percent"
        case memoryPercent = "memory_percent"
    }
}

struct Memory: Codable, Equatable, Sendable {
    let usedBytes: UInt64
    let freeBytes: UInt64
    let totalBytes: UInt64

    enum CodingKeys: String, CodingKey {
        case usedBytes = "used_bytes"
        case freeBytes = "free_bytes"
        case totalBytes = "total_bytes"
    }

    var usedPercent: Float {
        guard totalBytes > 0 else { return 0 }
        return Float(usedBytes) / Float(totalBytes) * 100.0
    }
}

struct GPUProcess: Codable, Equatable, Sendable {
    let pid: UInt32
    let name: String
    let usedMemoryBytes: UInt64
    let kind: ProcessKind

    enum CodingKeys: String, CodingKey {
        case pid
        case name
        case usedMemoryBytes = "used_memory_bytes"
        case kind = "type"
    }
}

enum ProcessKind: String, Codable, Sendable {
    case compute
    case graphics
    case mixed
}
