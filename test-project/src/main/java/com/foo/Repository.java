package com.foo;

public interface Repository {
    void save(Person p);
    Person findById(int id);
}
