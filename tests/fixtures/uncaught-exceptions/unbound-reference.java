package p;

import java.io.IOException;

public class A {
    interface Action { void apply(A receiver); }
    void read() throws IOException {}
    void run() {
        Action r = receiver -> {
            try {
                receiver.read();
            } catch (IOException e) {
                // TODO Auto-generated catch block
                e.printStackTrace();
            }
        };
    }
}
