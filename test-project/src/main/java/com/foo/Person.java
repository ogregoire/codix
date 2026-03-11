package com.foo;

public record Person(String name, int age) {
    Person {
        if (age < 0) throw new IllegalArgumentException("age must be non-negative");
    }
}
