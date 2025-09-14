```
NOTE: trace path: trace.log
NOTE: block path: block.log
WARN: len mismatch: block=8898 trace=8698
WARN: skipping 2593 matching lines

thread 'main' panicked at src/bin/analyser.rs:55:13:
assertion failed: `(left == right)`

Diff < left / right > :
 OpcodeTrace {
     pc: 2332,
     op: 80,
     name: "POP",
<    gas_used: 993,
>    gas_used: 0,
     gas_cost: 2,
     gas_back: 0,
     stack: [
         2835717307,
         944,
         688711508633122346260471332793165302056487531954,
         346500000000000000,
         0,
         3035,
         331497450208276499761800402457369371151716489247,
         688711508633122346260471332793165302056487531954,
         346500000000000000,
         0,
     ],
     memory: [
         331497450208276499761800402457369371151716489247,
         3,
         96,
     ],
     depth: 3,
 }
```
