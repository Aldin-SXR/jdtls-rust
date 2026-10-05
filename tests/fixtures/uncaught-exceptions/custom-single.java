package p;

public class A {
    static class IOException {}
    void read() throws java.io.IOException {}
    void run() {
        try {
            read();
        } catch (java.io.IOException failure) {
            // java.io.IOException failure in A.run
            failure.printStackTrace();
        }
    }
}
