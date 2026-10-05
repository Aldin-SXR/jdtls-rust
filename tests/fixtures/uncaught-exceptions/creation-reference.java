package p;

import java.io.IOException;
import java.util.function.Supplier;

public class A {
    A() throws IOException {}
    void run() {
        Supplier<A> r = () -> {
            try {
                return new A();
            } catch (IOException e) {
                // TODO Auto-generated catch block
                e.printStackTrace();
            }
        };
    }
}
