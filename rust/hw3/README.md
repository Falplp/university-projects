# Assignment for week 3

You can choose whether you want to work on more "unit-test" style tasks, or whether you want to build a larger application (or both :) ).

## Unit tests
The unit-test style tasks are below:

- Implement logic described in files in the `tests` directory.
- Implement logic described in the `src/encrypt_decrypt.rs` file.

## CPU emulator
If you want to work on something more interactive, we will be building a simple CPU emulator for a basic assembly language, which we will then use to interpret shaders to draw graphics on the screen.

From a high-level point of view, here is what the emulator should be able to do at start:
- Represent a simple computer:
  - 16 `u8` registers
  - 4 KiB of memory
  - Instruction pointer
  - Reading data from cells that were not written to previously should result in an error (otherwise we would be reading undefined memory).
  - Reading from a register index that doesn't exist, or from a memory address that is out of bounds should result in an error.
- Represent an assembly instruction set with basic operations:
  - `MOV A, B`: move a word from expression `A` to place `B`
  - `SET A, B`: set place `A` to expression `B`
  - `ADD A, B`: add expression `B` to place `A`
  - `SUB A, B`: subtract expression `B` from place `A`
  - `PRINT A`: print expression `A`
- An expression can be one of:
  - Numeric constant
  - Value of a register with the given index
  - Value of a memory cell with the given address
- A place can be one of:
  - Index of a register
  - Address of a memory cell

Implement a parser that will take a string containing multiple assembly instructions, and parse a program out of it, which can be then executed on the emulator.

Make sure to modularize the computer into several modules, based on area of responsibility.

Over the next few weeks, we will be extending the feature set of this emulator, so that it can actually be used to write simple shaders.
