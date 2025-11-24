#!/bin/sh

grep "###" etc/*.txt | cut -d ' ' -f 2,3 | sort > etc/todo.tmp
while read -r line; do etc/ckn.sh $line; done < etc/todo.tmp > etc/todo.txt
