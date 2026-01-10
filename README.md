# Hoot

A desktop email/messaging client built on the Nostr protocol using Rust and egui.
s
## Overview

Hoot is a native GUI application that provides email-like functionality over the Nostr protocol. It features a modern interface for sending and receiving messages, managing contacts, and viewing profiles.

## Building

### Prerequisites

- Rust 1.70 or later
- OpenSSL development libraries
- Perl and some libraries (probably)

### Build from Source

```bash
# Clone the repository
git clone <repository-url>
cd hoot

# Build the project
cargo build --release

# Run the application
cargo run --release
```

### Development Build

```bash
cargo build
cargo run
```

### Using Nix

If you have Nix with flakes enabled:

```bash
nix develop
```

## Development

### Profiling

Build with profiling support:

```bash
cargo build --features profiling
```

## License

See the project license file for details.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

## Author

Jack Chakany <jack@chakany.systems>
