package p;

import java.io.IOException;

public class A {
    String read() throws IOException { return null; }
    void run() {
        String value;
        try {
            value = read();
        } catch (IOException e) {
            // TODO Auto-generated catch block
            e.printStackTrace();
        }
        System.out.println(value);
    }
}
