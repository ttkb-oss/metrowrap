#include "common.h"

INCLUDE_RODATA("tests/data", OnlyRodata);

void use_rodata(void) {
    // Reference it so it doesn't get optimized away
    volatile const unsigned char* ptr = OnlyRodata;
}
