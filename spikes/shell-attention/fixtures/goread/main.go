// A Go binary that reads one line from standard input: built with
// CGO_ENABLED=0, so on Linux it is statically linked and makes its own
// syscalls, with no C library in between.
package main

import (
	"bufio"
	"fmt"
	"os"
)

func main() {
	fmt.Print("name? ")
	_, _ = bufio.NewReader(os.Stdin).ReadString('\n')
}
