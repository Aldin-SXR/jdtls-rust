package p;

import java.io.IOException;
import java.util.function.Supplier;

public class A {
    String read() throws IOException { return null; }
    void run() {
        Supplier<String> r = () -> {
            try {
                return read();
            } catch (IOException e) {
                // TODO Auto-generated catch block
                e.printStackTrace();
            }
        };
    }
}
