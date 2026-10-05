package p;

import java.io.IOException;

public class A {
    void read() throws IOException {}
    void run() {
        // move inside
        try {
            read(); // trailing
        } catch (IOException e) {
            // TODO Auto-generated catch block
            e.printStackTrace();
        }
    }
}
